//! Browser composition owned by the engine.

mod commands;
mod host;
mod host_state;
mod navigation;
mod panes;
mod system;

pub use host::WebWaiter;
pub use system::SystemPortScan;

use crate::{
    core::Core,
    errors::{self, Result},
    ports::PortScanPort,
};
use serde_json::Value;
use soyeht_protocol::{LocalServer, ServersLocalListResult};
use soyeht_web::LocalServerScan;

const REFRESH_INTERVAL_MS: i64 = 5_000;

pub(crate) fn handles_host_update(method: &str) -> bool {
    matches!(method, "web.host.navigated" | "web.host.console_counts")
}

pub(crate) use commands::handles as handles_command;

pub struct Service {
    scan: Box<dyn PortScanPort>,
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
            scan,
            servers: Vec::new(),
            next_refresh_ms: 0,
        }
    }

    pub fn refresh_if_due(&mut self, now_ms: i64) -> Result<Option<Refresh>> {
        if now_ms < self.next_refresh_ms {
            return Ok(None);
        }
        self.refresh(now_ms).map(Some)
    }

    pub fn refresh(&mut self, now_ms: i64) -> Result<Refresh> {
        self.next_refresh_ms = now_ms.saturating_add(REFRESH_INTERVAL_MS);
        let observation = self.scan.scan().map_err(errors::io)?;
        let servers =
            LocalServerScan::parse_lsof(&observation.listeners, &observation.directories, &[])?
                .into_servers();
        let changed = servers != self.servers;
        self.servers.clone_from(&servers);
        Ok(Refresh { servers, changed })
    }

    pub fn servers(&self) -> &[LocalServer] {
        &self.servers
    }
}

impl Core {
    pub fn local_servers_list(&mut self) -> Result<Value> {
        let now_ms = self.now_ms();
        if let Some(refresh) = self.web.refresh_if_due(now_ms)? {
            self.publish_local_server_change(&refresh)?;
        }
        serde_json::to_value(ServersLocalListResult {
            servers: self.web.servers().to_vec(),
        })
        .map_err(|_| errors::internal("Local server list serialization failed"))
    }

    pub(crate) fn advance_local_servers(&mut self) -> Result<()> {
        let now_ms = self.now_ms();
        if let Some(refresh) = self.web.refresh_if_due(now_ms)? {
            self.publish_local_server_change(&refresh)?;
        }
        Ok(())
    }

    fn publish_local_server_change(&mut self, refresh: &Refresh) -> Result<()> {
        if refresh.changed {
            self.transient_event(
                "servers.local.changed",
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
        dispatch::Outcome, outbox, ports::PortScanObservation, ports::testing::FixedClock,
    };
    use serde_json::json;
    use soyeht_protocol::frame::Frame;
    use std::{collections::VecDeque, fs, io, sync::Arc};
    use uuid::Uuid;

    struct ScriptedScan {
        observations: VecDeque<PortScanObservation>,
        calls: usize,
    }

    impl PortScanPort for ScriptedScan {
        fn scan(&mut self) -> io::Result<PortScanObservation> {
            self.calls += 1;
            self.observations
                .pop_front()
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
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
        let first = service.refresh_if_due(100).unwrap().unwrap();
        assert!(first.changed);
        assert_eq!(first.servers[0].command, "node");
        assert!(service.refresh_if_due(5_099).unwrap().is_none());
        assert!(!service.refresh_if_due(5_100).unwrap().unwrap().changed);
        assert!(service.refresh_if_due(10_100).unwrap().unwrap().changed);
        assert_eq!(service.servers()[0].command, "python");
    }

    #[test]
    fn engine_lists_cached_servers_and_publishes_only_normalized_changes() {
        let directory = std::env::temp_dir().join(format!("soyeht-web-engine-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        core.clock = Arc::new(FixedClock(100));
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
        let connection = core.connect(true, outbox).unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"server-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        let request =
            |id| json!({"jsonrpc":"2.0","id":id,"method":"servers.local.list","params":{}});
        let Outcome::Reply(first) = core.dispatch(&connection, request(2)).unwrap() else {
            panic!("local server list did not reply")
        };
        assert_eq!(first["servers"][0]["command"], "node");
        let Outcome::Reply(cached) = core.dispatch(&connection, request(3)).unwrap() else {
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
        let connection = core.connect(true, outbox).unwrap();
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
