//! The OpenCode meter follows the provider of the pane in use: when it is read, and what it
//! shows after the pane moves to another provider.

use super::Service;
use soyeht_protocol::{UsageProvider, UsageState};
use soyeht_usage::{ReadHints, host::*};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// A machine whose only reader that answers is OpenCode's activity, and which notes the provider
/// each activity read was for.
struct PaneHost {
    activity: Mutex<LocalUsageRead>,
    asked: Mutex<Vec<Option<String>>>,
}

impl PaneHost {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            activity: Mutex::new(LocalUsageRead::Totals(LocalUsage {
                tokens: 10,
                cost_usd: 0.0,
            })),
            asked: Mutex::default(),
        })
    }

    fn asked(&self) -> Vec<Option<String>> {
        self.asked.lock().unwrap().clone()
    }
}

impl Host for PaneHost {
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
        std::env::temp_dir()
    }
    fn read_file(&self, _path: &Path, _max_bytes: u64) -> FileRead {
        FileRead::Missing
    }
    fn keychain_password(&self, _service: &str, _account: Option<&str>) -> SecretRead {
        SecretRead::Unsupported
    }
    fn http(&self, _request: &HttpRequest) -> HttpResult {
        HttpResult::TransportFailed
    }
    fn browser_cookies(&self, _domain: &str) -> CookieRead {
        CookieRead::default()
    }
    fn state_db_value(&self, _path: &Path, _key: &str) -> SecretRead {
        SecretRead::Missing
    }
    fn program_found(&self, program: &str) -> bool {
        program == "opencode"
    }
    fn opencode_credential(&self, _database: &Path, _integration: &str) -> StoredCredential {
        StoredCredential::NoTable
    }
    fn opencode_usage(
        &self,
        _database: &Path,
        _since_ms: i64,
        provider: Option<&str>,
    ) -> LocalUsageRead {
        self.asked.lock().unwrap().push(provider.map(str::to_owned));
        self.activity.lock().unwrap().clone()
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

fn pane_uses(model: &str) -> ReadHints {
    ReadHints {
        opencode_model: Some(model.to_owned()),
    }
}

/// One background pass at `now_sec`: the providers it read, if it read any.
fn pass(service: &mut Service, now_sec: u64, hints: ReadHints) -> Option<Vec<UsageProvider>> {
    let prepared = service.prepare_automatic(now_sec, "then", hints)?;
    let providers = prepared.providers().to_vec();
    service.commit(prepared.read());
    Some(providers)
}

#[test]
fn a_pane_that_moves_opencode_to_another_provider_is_read_ahead_of_the_interval() {
    let host = PaneHost::new();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    assert_eq!(
        pass(&mut service, 1_000, ReadHints::default())
            .unwrap()
            .len(),
        4
    );
    assert_eq!(host.asked(), [None]);

    let moved = pane_uses("opencode-go/glm-5.3-flash");
    // Not hurried past the limit of a click, and not read again while nothing changed.
    assert_eq!(pass(&mut service, 1_010, moved.clone()), None);
    assert_eq!(pass(&mut service, 1_100, ReadHints::default()), None);
    assert_eq!(
        pass(&mut service, 1_100, moved.clone()),
        Some(vec![UsageProvider::Opencode])
    );
    assert_eq!(host.asked(), [None, Some("opencode-go".to_owned())]);

    // Another model of the same provider follows nothing new; closing the pane does.
    let sibling = pass(&mut service, 1_200, pane_uses("opencode-go/glm-5.3"));
    assert_eq!(sibling, None);
    assert_eq!(
        pass(&mut service, 1_200, ReadHints::default()),
        Some(vec![UsageProvider::Opencode])
    );
}

#[test]
fn a_provider_that_has_not_been_read_yet_is_read_by_the_ordinary_schedule() {
    let host = PaneHost::new();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    let first = pass(&mut service, 1_000, pane_uses("openai/gpt-5")).unwrap();
    assert!(first.contains(&UsageProvider::Opencode));
    assert_eq!(host.asked(), [Some("openai".to_owned())]);
}

#[test]
fn a_failed_read_of_the_new_provider_does_not_keep_the_old_providers_reading() {
    let host = PaneHost::new();
    let mut service = Service::restore(Vec::new(), host.clone(), true);
    pass(&mut service, 1_000, pane_uses("openai/gpt-5")).unwrap();
    let before = service.shown(&UsageProvider::Opencode, 1_000);
    assert_eq!(before.state, UsageState::Ok);

    *host.activity.lock().unwrap() = LocalUsageRead::NoHistory;
    pass(&mut service, 1_100, pane_uses("anthropic/claude")).unwrap();
    let after = service.shown(&UsageProvider::Opencode, 1_100);
    assert_eq!(after.state, UsageState::Unavailable);
    assert_eq!(after.activity, None);

    // Without a change the first failure after a good reading is still not shown.
    *host.activity.lock().unwrap() = LocalUsageRead::Totals(LocalUsage {
        tokens: 5,
        cost_usd: 0.0,
    });
    pass(&mut service, 1_500, pane_uses("anthropic/claude")).unwrap();
    *host.activity.lock().unwrap() = LocalUsageRead::NoHistory;
    pass(&mut service, 1_900, pane_uses("anthropic/claude")).unwrap();
    assert_eq!(
        service.shown(&UsageProvider::Opencode, 1_900).state,
        UsageState::Ok
    );
}

#[tokio::test]
async fn the_schedule_is_woken_and_not_otherwise() {
    let service = Service::restore(Vec::new(), PaneHost::new(), true);
    let woken = service.woken();
    let quiet = tokio::time::timeout(Duration::from_millis(50), woken.notified()).await;
    assert!(quiet.is_err());
    service.wake();
    let signalled = tokio::time::timeout(Duration::from_secs(5), woken.notified()).await;
    assert!(signalled.is_ok());
}
