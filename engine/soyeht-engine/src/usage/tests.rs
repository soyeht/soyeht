//! Engine-level usage tests over a scripted host: no network, credential or provider process.

use super::*;
use crate::{dispatch::Outcome, ports::testing::FixedClock};
use serde_json::json;
use soyeht_protocol::{UsageProvider, UsageSource, UsageState, UsageUnavailableReason};
use soyeht_usage::host::*;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const CODEX_LOGIN: &[u8] = br#"{"OPENAI_API_KEY":"sk-recorded-fixture"}"#;

/// Codex reads its API key and gets the scripted HTTP answers; Claude and Cursor find nothing.
struct ScriptedHost {
    http: Mutex<VecDeque<HttpResult>>,
}

fn usage_answer(percent: u32) -> HttpResult {
    HttpResult::Response {
        status: 200,
        retry_after_sec: None,
        body: format!(
            r#"{{"rate_limit":{{"primary_window":{{"used_percent":{percent},"reset_at":1800000000,"limit_window_seconds":18000}}}}}}"#
        )
        .into_bytes(),
    }
}

impl Host for ScriptedHost {
    fn platform(&self) -> Platform {
        Platform::Linux
    }
    fn env(&self, _name: &str) -> Option<String> {
        None
    }
    fn home(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/home/fixture"))
    }
    fn temp_dir(&self) -> PathBuf {
        PathBuf::from("/tmp/fixture")
    }
    fn read_file(&self, path: &Path, _max_bytes: u64) -> FileRead {
        if path.ends_with(".codex/auth.json") {
            FileRead::Bytes(CODEX_LOGIN.to_vec())
        } else {
            FileRead::Missing
        }
    }
    fn keychain_password(&self, _service: &str, _account: Option<&str>) -> SecretRead {
        SecretRead::Unsupported
    }
    fn http(&self, _request: &HttpRequest) -> HttpResult {
        self.http
            .lock()
            .ok()
            .and_then(|mut queue| queue.pop_front())
            .unwrap_or(HttpResult::TransportFailed)
    }
    fn browser_cookies(&self, _domain: &str) -> CookieRead {
        CookieRead::default()
    }
    fn state_db_value(&self, _path: &Path, _key: &str) -> SecretRead {
        SecretRead::Missing
    }
    fn json_rpc(&self, _request: &RpcRequest) -> RpcRun {
        RpcRun::ToolNotFound
    }
    fn run_terminal(
        &self,
        _request: &TerminalRequest,
        _driver: &mut dyn TerminalDriver,
    ) -> TerminalRun {
        TerminalRun::ToolNotFound
    }
}

fn service(answers: Vec<HttpResult>) -> Service {
    Service::restore(
        Vec::new(),
        Arc::new(ScriptedHost {
            http: Mutex::new(answers.into()),
        }),
        true,
    )
}

struct Engine {
    core: Core,
    connection: String,
    directory: PathBuf,
}

impl Engine {
    fn open(answers: Vec<HttpResult>, now: i64) -> Self {
        let directory = std::env::temp_dir().join(format!("sn-engine-usage-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        core.clock = Arc::new(FixedClock(now));
        core.usage = service(answers);
        let (outbox, _receiver) = crate::outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"usage-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        Self {
            core,
            connection,
            directory,
        }
    }

    fn click(&mut self, provider: &str, key: &str) -> Value {
        let params = json!({"provider": provider, "idempotencyKey": key});
        let prepared = self
            .core
            .usage_refresh_prepare(&self.connection, &params)
            .unwrap();
        let (finish, batch) = prepared.read();
        self.core.usage_refresh_finish(finish, batch).unwrap()
    }

    fn background(&mut self) {
        if let Some(prepared) = self.core.usage_automatic_prepare() {
            let batch = prepared.read();
            self.core.usage_automatic_commit(batch).unwrap();
        }
    }

    fn shown(&self, index: usize) -> Value {
        self.core.usage_list().unwrap()["providers"][index].clone()
    }

    fn close(self) {
        drop(self.core);
        fs::remove_dir_all(self.directory).unwrap();
    }
}

#[test]
fn list_is_closed_to_the_three_providers_and_starts_unchecked() {
    let listed = service(Vec::new()).providers();
    assert_eq!(
        listed,
        vec![
            UsageProvider::Claude,
            UsageProvider::Codex,
            UsageProvider::Cursor
        ]
    );
    let shown = service(Vec::new()).shown(&UsageProvider::Codex, 100);
    assert_eq!(shown.state, UsageState::Unavailable);
    assert_eq!(
        shown.unavailable_reason,
        Some(UsageUnavailableReason::NotChecked)
    );
    assert_eq!(shown.source, UsageSource::CodexAccessToken);
}

#[test]
fn a_click_is_read_outside_the_lock_stored_published_and_limited() {
    let mut engine = Engine::open(vec![usage_answer(25)], 100_000_000);
    let result = engine.click("codex", "refresh-once");
    assert_eq!(result["providers"][0]["primary"]["used"], 0.25);
    assert_eq!(result["providers"][0]["source"], "codex_account_endpoint");
    let stored = engine
        .core
        .store
        .usage_reading(&UsageProvider::Codex)
        .unwrap()
        .unwrap();
    assert_eq!(stored.reading.primary.unwrap().used, 0.25);
    let params = json!({"provider": "codex", "idempotencyKey": "too-soon"});
    let Err(error) = engine
        .core
        .usage_refresh_prepare(&engine.connection, &params)
    else {
        panic!("a second click inside 30 seconds was read");
    };
    let wire = crate::errors::wire(error);
    assert_eq!(wire.data.kind, soyeht_protocol::ErrorKind::RateLimited);
    assert_eq!(wire.data.retry_after_sec, Some(30));
    let repeated = engine.core.dispatch(
        &engine.connection,
        json!({"jsonrpc":"2.0","id":9,"method":"usage.refresh","params":{"provider":"codex","idempotencyKey":"refresh-once"}}),
    );
    assert!(matches!(repeated, Ok(Outcome::Reply(value)) if value == result));
    engine.close();
}

#[test]
fn the_background_schedule_reads_at_startup_and_after_five_minutes() {
    let mut engine = Engine::open(vec![usage_answer(10), usage_answer(20)], 100_000_000);
    engine.background();
    assert_eq!(engine.shown(1)["primary"]["used"], 0.1);
    assert_eq!(engine.shown(0)["state"], "unavailable");
    engine.core.clock = Arc::new(FixedClock(100_299_000));
    engine.background();
    assert_eq!(engine.shown(1)["primary"]["used"], 0.1);
    engine.core.clock = Arc::new(FixedClock(100_300_000));
    engine.background();
    assert_eq!(engine.shown(1)["primary"]["used"], 0.2);
    let events = engine.core.store.replay(0).unwrap().1;
    assert!(events.iter().any(|event| event.method == "usage.changed"));
    engine.close();
}

#[test]
fn failures_keep_the_last_good_reading_with_its_age_and_reason() {
    let mut engine = Engine::open(
        vec![
            usage_answer(40),
            HttpResult::TransportFailed,
            HttpResult::TransportFailed,
        ],
        100_000_000,
    );
    engine.click("codex", "good");
    engine.core.clock = Arc::new(FixedClock(100_031_000));
    let first = engine.click("codex", "first-failure");
    assert_eq!(
        first["providers"][0]["state"], "ok",
        "a first failure is not shown"
    );
    engine.core.clock = Arc::new(FixedClock(100_062_000));
    let second = engine.click("codex", "second-failure");
    let kept = &second["providers"][0];
    assert_eq!(kept["state"], "stale");
    assert_eq!(kept["primary"]["used"], 0.4);
    assert_eq!(kept["unavailableReason"], "network_error");
    assert_eq!(kept["fetchedAt"], first["providers"][0]["fetchedAt"]);
    let stored = engine
        .core
        .store
        .usage_reading(&UsageProvider::Codex)
        .unwrap()
        .unwrap();
    assert_eq!(stored.reading.primary.unwrap().used, 0.4);
    engine.close();
}

#[test]
fn restored_readings_are_last_known_until_read_again() {
    let mut engine = Engine::open(vec![usage_answer(30)], 100_000_000);
    engine.click("codex", "store-it");
    let directory = engine.directory.clone();
    drop(engine.core);
    let restored = Core::open(&directory).unwrap();
    let reading = &restored.usage_list().unwrap()["providers"][1];
    assert_eq!(reading["state"], "stale");
    assert_eq!(reading["primary"]["used"], 0.3);
    drop(restored);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn readings_never_carry_a_credential() {
    let mut engine = Engine::open(vec![usage_answer(5)], 100_000_000);
    let result = engine.click("codex", "secret-check");
    assert!(!result.to_string().contains("sk-recorded-fixture"));
    let events = engine.core.store.replay(0).unwrap().1;
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("sk-recorded-fixture")
    );
    engine.close();
}
