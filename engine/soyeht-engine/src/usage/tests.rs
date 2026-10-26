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
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

const CODEX_LOGIN: &[u8] = br#"{"OPENAI_API_KEY":"sk-recorded-fixture"}"#;

/// Codex reads its API key and gets the scripted HTTP answers; Cursor finds nothing. Claude finds
/// no login until `claude_login` is set, and then reads the same usage whatever its endpoint is asked.
struct ScriptedHost {
    http: Mutex<VecDeque<HttpResult>>,
    /// When Claude Code's credentials file exists: the expiry of its login, in milliseconds.
    claude_login: Mutex<Option<u64>>,
    /// A stand-in for the `claude` program, run in the real pseudo terminal: where it notes what
    /// it saw. Without it there is no such program.
    fake_cli: Option<FakeCli>,
    stopped_in: Mutex<Option<Duration>>,
    /// The scratch directory sessions run in. A session's directory has a fixed name inside it, so
    /// two hosts sharing one would remove each other's directory under a running child.
    temp: PathBuf,
}

impl ScriptedHost {
    fn new(answers: Vec<HttpResult>) -> Self {
        Self {
            http: Mutex::new(answers.into()),
            claude_login: Mutex::new(None),
            fake_cli: None,
            stopped_in: Mutex::new(None),
            temp: std::env::temp_dir(),
        }
    }

    fn sign_in_claude(&self, expires_ms: u64) {
        *self.claude_login.lock().unwrap() = Some(expires_ms);
    }
}

const CLAUDE_USAGE: &[u8] =
    br#"{"five_hour":{"utilization":10,"resets_at":"2030-01-01T00:00:00Z"}}"#;

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
        self.temp.clone()
    }
    fn read_file(&self, path: &Path, _max_bytes: u64) -> FileRead {
        let claude = *self.claude_login.lock().unwrap();
        match claude {
            Some(expires) if path.ends_with(".claude/.credentials.json") => FileRead::Bytes(
                format!(r#"{{"claudeAiOauth":{{"accessToken":"sk-ant-oat01-x","refreshToken":"sk-ant-ort01-x","expiresAt":{expires},"scopes":["user:profile"]}}}}"#)
                    .into_bytes(),
            ),
            _ if path.ends_with(".codex/auth.json") => FileRead::Bytes(CODEX_LOGIN.to_vec()),
            _ => FileRead::Missing,
        }
    }
    fn keychain_password(&self, _service: &str, _account: Option<&str>) -> SecretRead {
        SecretRead::Unsupported
    }
    fn http(&self, request: &HttpRequest) -> HttpResult {
        if request.url.contains("anthropic.com") {
            return HttpResult::Response {
                status: 200,
                retry_after_sec: None,
                body: CLAUDE_USAGE.to_vec(),
            };
        }
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
        request: &TerminalRequest,
        driver: &mut dyn TerminalDriver,
    ) -> TerminalRun {
        let Some(cli) = &self.fake_cli else {
            return TerminalRun::ToolNotFound;
        };
        assert_eq!(request.program, "claude");
        let (program, args) = cli.command();
        let fake = TerminalRequest {
            program: program.clone(),
            args,
            env: request.env.clone(),
            workdir: request.workdir.clone(),
            files: request.files.clone(),
            transcript: request.transcript.clone(),
            timeout: Duration::from_secs(5),
        };
        let (shell, path_env) = if cfg!(windows) {
            (
                PathBuf::from(r"C:\Windows\System32\cmd.exe"),
                r"C:\Windows\System32",
            )
        } else {
            (PathBuf::from("/bin/sh"), "/usr/bin:/bin")
        };
        let started = Instant::now();
        let outcome = terminal::run(&shell, path_env, &fake, driver);
        *self.stopped_in.lock().unwrap() = Some(started.elapsed());
        outcome
    }
}

fn service(answers: Vec<HttpResult>) -> Service {
    Service::restore(Vec::new(), Arc::new(ScriptedHost::new(answers)), true)
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
    assert_eq!(engine.shown(0)["state"], "login_needed");
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

const FAR: u64 = 99_999_999_999_999;

/// Reads whatever the background schedule wants at `now_sec` and says whether Claude was in it.
fn automatic_round(service: &mut Service, now_sec: u64) -> Option<bool> {
    let prepared = service.prepare_automatic(now_sec, "t")?;
    let claude = prepared.providers().contains(&UsageProvider::Claude);
    let committed = service.commit(prepared.read());
    if claude {
        let shown = committed
            .iter()
            .find(|item| item.shown.provider == UsageProvider::Claude)?;
        return Some(shown.log_signed_out);
    }
    Some(false)
}

fn claude_due(service: &mut Service, now_sec: u64) -> bool {
    service
        .prepare_automatic(now_sec, "t")
        .is_some_and(|prepared| {
            let due = prepared.providers().contains(&UsageProvider::Claude);
            service.abandon(prepared.providers());
            due
        })
}

#[test]
fn a_signed_out_provider_is_asked_again_at_doubling_intervals_until_it_signs_in() {
    let host = Arc::new(ScriptedHost::new(Vec::new()));
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    let start = 1_000_000;
    // Startup read: signed out, logged once.
    assert_eq!(automatic_round(&mut service, start), Some(true));
    assert_eq!(
        service.shown(&UsageProvider::Claude, start).state,
        UsageState::LoginNeeded
    );
    // After one failure the wait is twice the background interval, then four times, then the
    // longest backoff.
    let mut at = start;
    for wait in [600, 1_200, 1_800, 1_800] {
        assert!(
            !claude_due(&mut service, at + wait - 1),
            "due after {wait} - 1"
        );
        assert!(claude_due(&mut service, at + wait), "not due after {wait}");
        automatic_round(&mut service, at + wait);
        at += wait;
    }
    // Signing in ends the streak at the next read; the schedule is back at five minutes.
    host.sign_in_claude(FAR);
    at += 1_800;
    automatic_round(&mut service, at);
    assert_eq!(
        service.shown(&UsageProvider::Claude, at).state,
        UsageState::Ok
    );
    assert!(!claude_due(&mut service, at + 299));
    assert!(claude_due(&mut service, at + 300));
}

#[test]
fn a_click_reads_a_signed_out_provider_at_once() {
    let host = Arc::new(ScriptedHost::new(Vec::new()));
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    assert_eq!(automatic_round(&mut service, 1_000_000), Some(true));
    host.sign_in_claude(FAR);
    let prepared = service
        .prepare_manual(Some(UsageProvider::Claude), 1_000_010, "t")
        .ok()
        .unwrap();
    service.commit(prepared.read());
    assert_eq!(
        service.shown(&UsageProvider::Claude, 1_000_010).state,
        UsageState::Ok
    );
}

const NOW: u64 = 1_790_000_000;
const EXPIRED: u64 = 1;

/// A stand-in for Claude Code that is not signed in. After `delay_sec` of a blank screen it shows
/// `lines`, notes the `BROWSER` it was given, and "opens the browser" (a marker) as soon as any
/// key reaches it, before or after the screen is drawn.
#[derive(Clone)]
struct FakeCli {
    marker: PathBuf,
    delay_sec: u64,
    lines: &'static [&'static str],
}

/// The menu a signed-out Claude Code shows today, and one with wording the driver has never seen.
const KNOWN_MENU: &[&str] = &[
    "Select login method:",
    "1. Claude account with subscription",
];
const NEW_WORDING_MENU: &[&str] = &["Sign in to continue", "1. Use your Claude subscription"];

impl FakeCli {
    fn command(&self) -> (String, Vec<String>) {
        let marker = self.marker.display();
        let delay = self.delay_sec;
        if cfg!(windows) {
            let screen: String = self
                .lines
                .iter()
                .map(|line| format!(" & echo {line}"))
                .collect();
            (
                "cmd.exe".into(),
                vec![
                    "/c".into(),
                    format!(
                        "echo started > {marker}\\started & echo %BROWSER% > {marker}\\browser & ping -n {} 127.0.0.1 > nul{screen} & set /p key= & echo opened > {marker}\\opened & ping -n 30 127.0.0.1 > nul",
                        delay + 1
                    ),
                ],
            )
        } else {
            let screen: String = self.lines.iter().map(|line| format!("{line}\\n")).collect();
            (
                "sh".into(),
                vec![
                    "-c".into(),
                    format!(
                        "echo started > '{marker}/started'; echo \"$BROWSER\" > '{marker}/browser'; sleep {delay}; printf '{screen}'; IFS= read -r key; echo opened > '{marker}/opened'; sleep 30"
                    ),
                ],
            )
        }
    }
}

/// A machine with the fake Claude Code; `login` is the expiry of the login file it holds, if any.
fn machine_with_fake_cli(
    login: Option<u64>,
    delay_sec: u64,
    lines: &'static [&'static str],
) -> (Arc<ScriptedHost>, PathBuf) {
    let marker = std::env::temp_dir().join(format!("sn-probe-{}", Uuid::new_v4()));
    fs::create_dir_all(&marker).unwrap();
    let host = ScriptedHost {
        temp: marker.clone(),
        fake_cli: Some(FakeCli {
            marker: marker.clone(),
            delay_sec,
            lines,
        }),
        ..ScriptedHost::new(Vec::new())
    };
    if let Some(expires) = login {
        host.sign_in_claude(expires);
    }
    (Arc::new(host), marker)
}

fn read_claude(host: &Arc<ScriptedHost>) -> soyeht_usage::RefreshOutcome {
    soyeht_usage::Planner::new(host.clone()).refresh(
        &UsageProvider::Claude,
        soyeht_usage::Trigger::Manual,
        NOW,
        "t",
    )
}

#[test]
fn a_signed_out_machine_never_starts_claude_code() {
    let (host, marker) = machine_with_fake_cli(None, 0, KNOWN_MENU);
    let outcome = read_claude(&host);
    assert_eq!(outcome.reading.state, UsageState::LoginNeeded);
    assert!(!marker.join("started").exists(), "the program was started");
    fs::remove_dir_all(marker).unwrap();
}

#[test]
fn a_session_at_the_login_menu_is_stopped_without_a_key_and_with_no_browser() {
    let (host, marker) = machine_with_fake_cli(Some(EXPIRED), 0, KNOWN_MENU);
    let outcome = read_claude(&host);
    assert_eq!(outcome.reading.state, UsageState::LoginNeeded);
    assert!(marker.join("started").exists());
    assert!(
        !marker.join("opened").exists(),
        "a key reached the login menu"
    );
    #[cfg(unix)]
    {
        let browser = fs::read_to_string(marker.join("browser")).unwrap();
        assert_eq!(browser.trim(), process::NO_BROWSER);
    }
    let stopped_in = host.stopped_in.lock().unwrap().unwrap();
    assert!(stopped_in < Duration::from_secs(8), "{stopped_in:?}");
    fs::remove_dir_all(marker).unwrap();
}

#[test]
fn a_screen_with_new_wording_or_a_late_menu_gets_no_key_and_no_browser() {
    // The menu is drawn at once, in words the driver does not know; or after 3 s of a blank
    // screen, when a driver that types on a timer would already have sent its command.
    for (delay_sec, lines) in [
        (0, NEW_WORDING_MENU),
        (3, NEW_WORDING_MENU),
        (3, KNOWN_MENU),
    ] {
        let (host, marker) = machine_with_fake_cli(Some(EXPIRED), delay_sec, lines);
        let outcome = read_claude(&host);
        assert_ne!(outcome.reading.state, UsageState::Ok);
        assert!(marker.join("started").exists());
        assert!(
            !marker.join("opened").exists(),
            "a key reached the menu ({delay_sec} s, {lines:?})"
        );
        fs::remove_dir_all(marker).unwrap();
    }
}

#[test]
fn a_click_that_finds_the_provider_still_signed_out_does_not_lengthen_the_hold() {
    let host = Arc::new(ScriptedHost::new(Vec::new()));
    let mut service = Service::restore(Vec::new(), host, true);
    let start = 1_000_000;
    assert_eq!(automatic_round(&mut service, start), Some(true));
    for click in 1..=3 {
        let at = start + click * 40;
        let prepared = service
            .prepare_manual(Some(UsageProvider::Claude), at, "t")
            .ok()
            .unwrap();
        let committed = service.commit(prepared.read());
        assert!(!committed[0].log_signed_out);
    }
    // One automatic read so far: the schedule waits twice the interval after it, not 30 min.
    let last_attempt = start + 120;
    assert!(!claude_due(&mut service, last_attempt + 599));
    assert!(claude_due(&mut service, last_attempt + 600));
}
