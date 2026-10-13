//! Browser composition owned by the engine.

mod commands;
mod host;
mod host_state;
mod navigation;
mod panes;
mod permissions;
mod system;

pub use host::WebWaiter;
pub use system::SystemPortScan;
#[cfg(unix)]
pub(crate) use system::lsof_program;

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::{Offload, SharedPort},
    ports::{PortScanObservation, PortScanPort},
};
use serde_json::Value;
use soyeht_protocol::{LocalServer, ServersLocalListResult, event, method};
use soyeht_web::LocalServerScan;
use std::time::Duration;

const REFRESH_INTERVAL_MS: i64 = 5_000;
const SCAN_JOB: &str = "local_servers";
/// Two bounded `lsof` runs, plus a wait behind a scan already running.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn handles_host_update(method: &str) -> bool {
    matches!(
        method,
        method::WEB_HOST_NAVIGATED | method::WEB_HOST_CONSOLE_COUNTS
    )
}

pub(crate) use commands::handles as handles_command;

pub struct Service {
    scan: SharedPort<dyn PortScanPort>,
    /// Read once, so asking never waits behind a scan that holds the port.
    unavailable: Option<&'static str>,
    servers: Vec<LocalServer>,
    next_refresh_ms: i64,
}

pub struct Refresh {
    pub servers: Vec<LocalServer>,
    pub changed: bool,
}

impl Service {
    pub fn new(scan: Box<dyn PortScanPort>) -> Self {
        Self {
            unavailable: scan.unavailable(),
            scan: SharedPort::new(scan),
            servers: Vec::new(),
            next_refresh_ms: 0,
        }
    }

    /// Claims the refresh due at `now_ms`, if any, and hands out the scanner
    /// to run off the Core lock.
    fn claim_refresh(&mut self, now_ms: i64) -> Option<SharedPort<dyn PortScanPort>> {
        if now_ms < self.next_refresh_ms {
            return None;
        }
        self.next_refresh_ms = now_ms.saturating_add(REFRESH_INTERVAL_MS);
        Some(self.scan.clone())
    }

    fn finish(&mut self, observation: PortScanObservation) -> Result<Refresh> {
        let servers =
            LocalServerScan::parse_lsof(&observation.listeners, &observation.directories, &[])?
                .into_servers();
        let changed = servers != self.servers;
        self.servers.clone_from(&servers);
        Ok(Refresh { servers, changed })
    }

    /// Why local servers cannot be listed on this system, if they cannot.
    pub fn unavailable(&self) -> Option<&'static str> {
        self.unavailable
    }

    pub fn servers(&self) -> &[LocalServer] {
        &self.servers
    }
}

/// Runs `lsof`; called off the Core lock.
fn scan(scanner: &SharedPort<dyn PortScanPort>) -> Result<PortScanObservation> {
    scanner.with(|port| port.scan().map_err(errors::Error::from))
}

impl Core {
    /// Answers from the cache, or scans off the lock first when a refresh is due.
    pub fn local_servers_list(&mut self) -> Result<Outcome> {
        if let Some(reason) = self.web.unavailable() {
            return Err(errors::platform_unsupported(reason));
        }
        let Some(scanner) = self.claim_local_server_scan() else {
            return self.cached_local_servers().map(Outcome::Reply);
        };
        Ok(Outcome::Offload(Offload::new(
            "Local server scan timed out",
            LIST_TIMEOUT,
            move || scan(&scanner),
            |core, observation| {
                core.record_local_servers(observation?)?;
                core.cached_local_servers().map(Outcome::Reply)
            },
        )))
    }

    pub(crate) fn advance_local_servers(&mut self) -> Result<()> {
        let Some(scanner) = self.claim_local_server_scan() else {
            return Ok(());
        };
        self.spawn_background(
            SCAN_JOB.into(),
            move || scan(&scanner),
            |core, observation| core.record_local_servers(observation?),
        )
    }

    fn claim_local_server_scan(&mut self) -> Option<SharedPort<dyn PortScanPort>> {
        // A system that cannot list servers is not scanned at all.
        if self.web.unavailable().is_some() || self.background.is_active(SCAN_JOB) {
            return None;
        }
        let now_ms = self.now_ms();
        self.web.claim_refresh(now_ms)
    }

    fn cached_local_servers(&self) -> Result<Value> {
        serde_json::to_value(ServersLocalListResult {
            servers: self.web.servers().to_vec(),
        })
        .map_err(|_| errors::internal("Local server list serialization failed"))
    }

    fn record_local_servers(&mut self, observation: PortScanObservation) -> Result<()> {
        let refresh = self.web.finish(observation)?;
        if refresh.changed {
            self.transient_event(
                event::SERVERS_LOCAL_CHANGED,
                serde_json::json!({"servers": refresh.servers}),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dispatch::Outcome,
        outbox,
        ports::{PortScanError, PortScanObservation, testing::FixedClock},
    };
    use serde_json::json;
    use soyeht_protocol::frame::Frame;
    use std::{collections::VecDeque, fs, sync::Arc};
    use uuid::Uuid;

    struct ScriptedScan {
        observations: VecDeque<PortScanObservation>,
        calls: usize,
    }

    impl PortScanPort for ScriptedScan {
        fn scan(&mut self) -> std::result::Result<PortScanObservation, PortScanError> {
            self.calls += 1;
            self.observations.pop_front().ok_or(PortScanError::Failed {
                code: Some(2),
                detail: "script exhausted".into(),
            })
        }
    }

    fn observation(command: &str) -> PortScanObservation {
        PortScanObservation {
            listeners: format!("p10\0c{command}\0n127.0.0.1:3000\0").into_bytes(),
            directories: b"p10\0fcwd\0n/tmp/site\0".to_vec(),
        }
    }

    #[test]
    fn refresh_is_due_every_five_seconds_and_reports_only_real_changes() {
        let mut service = Service::new(Box::new(ScriptedScan {
            observations: [
                observation("node"),
                observation("node"),
                observation("python"),
            ]
            .into(),
            calls: 0,
        }));
        let mut refresh_if_due = |now_ms| {
            service
                .claim_refresh(now_ms)
                .map(|scanner| service.finish(scan(&scanner).unwrap()).unwrap())
        };
        let first = refresh_if_due(100).unwrap();
        assert!(first.changed);
        assert_eq!(first.servers[0].command, "node");
        assert!(refresh_if_due(5_099).is_none());
        assert!(!refresh_if_due(5_100).unwrap().changed);
        assert!(refresh_if_due(10_100).unwrap().changed);
        assert_eq!(service.servers()[0].command, "python");
    }

    #[test]
    fn engine_lists_cached_servers_and_publishes_only_normalized_changes() {
        let directory = std::env::temp_dir().join(format!("soyeht-web-engine-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        core.clock = Arc::new(FixedClock(100));
        core.background = crate::offload::Background::inline();
        core.web = Service::new(Box::new(ScriptedScan {
            observations: [
                observation("node"),
                observation("node"),
                observation("python"),
            ]
            .into(),
            calls: 0,
        }));
        let (outbox, _receiver) = crate::outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"server-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        let request =
            |id| json!({"jsonrpc":"2.0","id":id,"method":"servers.local.list","params":{}});
        let Outcome::Reply(first) = core.dispatch_inline(&connection, request(2)).unwrap() else {
            panic!("local server list did not reply")
        };
        assert_eq!(first["servers"][0]["command"], "node");
        let Outcome::Reply(cached) = core.dispatch_inline(&connection, request(3)).unwrap() else {
            panic!("cached local server list did not reply")
        };
        assert_eq!(cached, first);
        assert_eq!(durable_server_event_count(&mut core), 0);
        core.clock = Arc::new(FixedClock(5_100));
        core.advance_local_servers().unwrap();
        assert_eq!(durable_server_event_count(&mut core), 0);
        core.clock = Arc::new(FixedClock(10_100));
        core.advance_local_servers().unwrap();
        assert_eq!(durable_server_event_count(&mut core), 0);
        assert_eq!(core.web.servers()[0].command, "python");
        drop(core);
        fs::remove_dir_all(directory).unwrap();
    }

    struct UnavailableScan {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl PortScanPort for UnavailableScan {
        fn unavailable(&self) -> Option<&'static str> {
            Some("fixture system cannot list servers")
        }

        fn scan(&mut self) -> std::result::Result<PortScanObservation, PortScanError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(PortScanError::Unsupported(
                "fixture system cannot list servers",
            ))
        }
    }

    #[test]
    fn a_system_that_cannot_list_servers_is_never_scanned_and_answers_with_a_typed_error() {
        let directory = std::env::temp_dir().join(format!("soyeht-web-none-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        core.web = Service::new(Box::new(UnavailableScan {
            calls: Arc::clone(&calls),
        }));
        // The poller schedules nothing: no error, and the scan is never called.
        core.advance_local_servers().unwrap();
        core.advance_local_servers().unwrap();
        let Err(error) = core.local_servers_list() else {
            panic!("a system that cannot list servers answered the list")
        };
        assert_eq!(
            errors::kind(&error),
            soyeht_protocol::ErrorKind::PlatformUnsupported
        );
        assert!(error.to_string().contains("cannot list servers"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        drop(core);
        fs::remove_dir_all(directory).unwrap();
    }

    fn durable_server_event_count(core: &mut Core) -> usize {
        core.store
            .replay(0)
            .unwrap()
            .1
            .iter()
            .filter(|event| event.method == "servers.local.changed")
            .count()
    }

    #[tokio::test]
    async fn transient_events_reach_live_subscribers_without_entering_replay() {
        let directory = std::env::temp_dir().join(format!("soyeht-web-event-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        let (outbox, mut receiver) = outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        let _ = receiver.next().await.unwrap().unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
                "client":"transient-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        core.clients.get_mut(&connection).unwrap().events = true;
        core.transient_event(
            "web.console_counts",
            json!({
                "paneId":"pane","warnings":2,"errors":1
            }),
        )
        .unwrap();
        let Frame::Json(bytes) = receiver.next().await.unwrap().unwrap().frame else {
            panic!("expected transient JSON event")
        };
        let event: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(event["method"], "web.console_counts");
        assert!(event["params"].get("eventId").is_none());
        assert_eq!(durable_server_event_count(&mut core), 0);
        assert!(core.transient_event("web.navigated", json!({})).is_err());
        assert!(
            core.transient_event("web.console_counts", json!(null))
                .is_err()
        );
        assert!(
            core.transient_event("web.console_counts", json!({"eventId":"1"}))
                .is_err()
        );
        drop(core);
        fs::remove_dir_all(directory).unwrap();
    }
}
