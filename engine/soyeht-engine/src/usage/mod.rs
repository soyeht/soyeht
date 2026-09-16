//! Subscription-usage composition, persistence and wire publication.

mod sources;

pub use sources::SystemSources;

use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::Value;
use soyeht_protocol::{
    UsageListResult, UsageProvider, UsageReading, UsageRefreshParams, UsageRefreshResult,
    UsageSource, UsageState, UsageWindow,
};
use soyeht_store::usage::StoredUsageReading;
use soyeht_usage::{
    ManualRefresh, ReaderRegistry, Reading, RefreshOutcome, RefreshPolicy, UsageSourcePort, Window,
};

pub struct Service {
    registry: ReaderRegistry,
    policy: RefreshPolicy,
    readings: Vec<Reading>,
    source: Box<dyn UsageSourcePort>,
}

#[derive(Debug)]
pub struct RefreshBatch {
    observed_at_sec: u64,
    outcomes: Vec<RefreshOutcome>,
}

impl Service {
    pub fn restore(stored: Vec<StoredUsageReading>, source: Box<dyn UsageSourcePort>) -> Self {
        let readings = stored
            .into_iter()
            .map(restored_reading)
            .map(|reading| reading.restored_from_store())
            .collect();
        Self {
            registry: ReaderRegistry::new(),
            policy: RefreshPolicy::default(),
            readings,
            source,
        }
    }

    pub fn list(&self, now_sec: u64) -> Vec<UsageReading> {
        self.registry
            .providers()
            .into_iter()
            .map(|provider| self.display_reading(provider, now_sec))
            .map(|reading| wire_reading(&reading))
            .collect()
    }

    pub fn read_refresh(
        &mut self,
        provider: Option<UsageProvider>,
        now_sec: u64,
        fetched_at: &str,
    ) -> Result<RefreshBatch> {
        let providers = provider.map_or_else(|| self.registry.providers(), |item| vec![item]);
        if let Some(retry_after_sec) = self.longest_retry(&providers, now_sec) {
            return Err(errors::rate_limited(
                "Usage refresh is limited per provider",
                retry_after_sec,
            ));
        }
        let mut outcomes = Vec::with_capacity(providers.len());
        for item in providers {
            outcomes.push(
                self.registry
                    .refresh(item, self.source.as_mut(), fetched_at, now_sec)
                    .map_err(errors::Error::Usage)?,
            );
        }
        Ok(RefreshBatch {
            observed_at_sec: now_sec,
            outcomes,
        })
    }

    pub fn commit(&mut self, batch: RefreshBatch) {
        for outcome in batch.outcomes {
            self.policy.record(
                outcome.reading.provider.clone(),
                batch.observed_at_sec,
                outcome.transient.as_ref(),
            );
            replace_reading(&mut self.readings, outcome.reading);
        }
    }

    fn longest_retry(&self, providers: &[UsageProvider], now_sec: u64) -> Option<u32> {
        providers
            .iter()
            .filter_map(
                |provider| match self.policy.manual(provider.clone(), now_sec) {
                    ManualRefresh::Ready => None,
                    ManualRefresh::RateLimited { retry_after_sec } => Some(retry_after_sec),
                },
            )
            .max()
    }

    fn display_reading(&self, provider: UsageProvider, now_sec: u64) -> Reading {
        self.readings
            .iter()
            .find(|reading| reading.provider == provider)
            .map(|reading| reading.for_display(now_sec))
            .unwrap_or_else(|| missing_reading(&self.registry, provider, now_sec))
    }
}

impl Core {
    pub fn usage_list(&self) -> Result<Value> {
        let now_sec = nonnegative_time(self.clock.now_unix());
        serde_json::to_value(UsageListResult {
            providers: self.usage.list(now_sec),
        })
        .map_err(|_| errors::internal("Usage list serialization failed"))
    }

    pub fn usage_refresh(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: UsageRefreshParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid usage refresh parameters"))?;
        let observed_at = self.clock.now_unix();
        let batch = self.usage.read_refresh(
            request.provider,
            nonnegative_time(observed_at),
            &self.clock.timestamp(),
        )?;
        let readings = batch
            .outcomes
            .iter()
            .map(|outcome| wire_reading(&outcome.reading))
            .collect::<Vec<_>>();
        let result = serde_json::to_value(UsageRefreshResult {
            providers: readings.clone(),
        })
        .map_err(|_| errors::internal("Usage refresh serialization failed"))?;
        let events = readings
            .iter()
            .map(|reading| Ok(("usage.changed".into(), serde_json::to_value(reading)?)))
            .collect::<serde_json::Result<Vec<_>>>()
            .map_err(|_| errors::internal("Usage event serialization failed"))?;
        let stored = batch
            .outcomes
            .iter()
            .filter(|outcome| outcome.reading.state == UsageState::Ok)
            .map(|outcome| wire_reading(&outcome.reading))
            .collect::<Vec<_>>();
        let mutation = self.mutate(connection, "usage.refresh", params, |change| {
            for reading in &stored {
                change.save_usage_reading(reading, observed_at)?;
            }
            Ok((result, events))
        })?;
        self.usage.commit(batch);
        Ok(mutation.result)
    }
}

fn nonnegative_time(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn replace_reading(readings: &mut Vec<Reading>, replacement: Reading) {
    if let Some(index) = readings
        .iter()
        .position(|reading| reading.provider == replacement.provider)
    {
        readings[index] = replacement;
    } else {
        readings.push(replacement);
    }
}

fn missing_reading(
    registry: &ReaderRegistry,
    provider: UsageProvider,
    observed_at_sec: u64,
) -> Reading {
    let source = registry
        .sources(provider.clone())
        .into_iter()
        .next()
        .unwrap_or(UsageSource::ClaudeCliUsage);
    Reading {
        provider,
        state: UsageState::Unavailable,
        primary: None,
        secondary: None,
        source,
        fetched_at: String::new(),
        observed_at_sec,
    }
}

fn restored_reading(stored: StoredUsageReading) -> Reading {
    Reading {
        provider: stored.reading.provider,
        state: stored.reading.state,
        primary: stored.reading.primary.map(domain_window),
        secondary: stored.reading.secondary.map(domain_window),
        source: stored.reading.source,
        fetched_at: stored.reading.fetched_at,
        observed_at_sec: nonnegative_time(stored.observed_at_unix_sec),
    }
}

fn domain_window(window: UsageWindow) -> Window {
    Window {
        used: window.used,
        resets_at: window.resets_at,
        label: window.label,
    }
}

fn wire_reading(reading: &Reading) -> UsageReading {
    UsageReading {
        provider: reading.provider.clone(),
        state: reading.state.clone(),
        primary: reading.primary.as_ref().map(wire_window),
        secondary: reading.secondary.as_ref().map(wire_window),
        source: reading.source.clone(),
        fetched_at: reading.fetched_at.clone(),
    }
}

fn wire_window(window: &Window) -> UsageWindow {
    UsageWindow {
        used: window.used,
        resets_at: window.resets_at.clone(),
        label: window.label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch::Outcome, errors, ports::testing::FixedClock};
    use serde_json::json;
    use soyeht_protocol::{UsageProvider, UsageSource, UsageState};
    use soyeht_usage::{SourceRead, UsageSourcePort};
    use std::{collections::VecDeque, fs, sync::Arc};
    use uuid::Uuid;

    struct Scripted {
        reads: VecDeque<SourceRead>,
        calls: usize,
    }

    impl UsageSourcePort for Scripted {
        fn read(&mut self, _provider: UsageProvider, _source: UsageSource) -> SourceRead {
            self.calls += 1;
            self.reads.pop_front().unwrap_or(SourceRead::Unavailable)
        }
    }

    fn payload(used: u32) -> SourceRead {
        SourceRead::Payload(
            format!(
                "{{\"result\":{{\"rateLimits\":{{\"primary\":{{\"usedPercent\":{used},\"resetsAt\":1800000000}}}}}}}}"
            )
            .into_bytes(),
        )
    }

    fn service(reads: Vec<SourceRead>) -> Service {
        Service::restore(
            Vec::new(),
            Box::new(Scripted {
                reads: reads.into(),
                calls: 0,
            }),
        )
    }

    #[test]
    fn list_is_closed_to_registered_providers_and_missing_values_are_unavailable() {
        let usage = service(Vec::new());
        let listed = usage.list(100);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].provider, UsageProvider::Claude);
        assert_eq!(listed[1].provider, UsageProvider::Codex);
        assert_eq!(listed[2].provider, UsageProvider::Cursor);
        assert!(
            listed
                .iter()
                .all(|item| item.state == UsageState::Unavailable)
        );
    }

    #[test]
    fn batch_rate_limit_fails_before_any_source_and_uses_the_longest_wait() {
        let mut usage = service(vec![SourceRead::Unavailable; 12]);
        let first = usage
            .read_refresh(Some(UsageProvider::Codex), 100, "now")
            .unwrap();
        usage.commit(first);
        let error = usage.read_refresh(None, 101, "later").unwrap_err();
        let wire = errors::wire(error);
        assert_eq!(wire.data.kind, soyeht_protocol::ErrorKind::RateLimited);
        assert_eq!(wire.data.retry_after_sec, Some(29));
    }

    #[test]
    fn restored_success_is_immediately_stale_and_keeps_both_windows() {
        let stored = StoredUsageReading {
            reading: UsageReading {
                provider: UsageProvider::Codex,
                state: UsageState::Ok,
                primary: Some(UsageWindow {
                    used: 0.4,
                    resets_at: "later".into(),
                    label: "Current".into(),
                }),
                secondary: Some(UsageWindow {
                    used: 0.7,
                    resets_at: "week".into(),
                    label: "Weekly".into(),
                }),
                source: UsageSource::CodexAppServer,
                fetched_at: "before".into(),
            },
            observed_at_unix_sec: 99,
        };
        let usage = Service::restore(
            vec![stored],
            Box::new(Scripted {
                reads: VecDeque::new(),
                calls: 0,
            }),
        );
        let item = &usage.list(100)[1];
        assert_eq!(item.state, UsageState::Stale);
        assert_eq!(item.primary.as_ref().unwrap().used, 0.4);
        assert_eq!(item.secondary.as_ref().unwrap().used, 0.7);
    }

    #[test]
    fn successful_refresh_is_durable_published_and_idempotent() {
        let directory = std::env::temp_dir().join(format!("sn-engine-usage-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        core.clock = Arc::new(FixedClock(100_000));
        core.usage = service(vec![payload(25)]);
        let (outbox, _receiver) = crate::outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"usage-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":2,"method":"events.subscribe","params":{"since":"0"}}),
        )
        .unwrap();
        let params = json!({"provider":"codex","idempotencyKey":"refresh-once"});
        let call = |id| json!({"jsonrpc":"2.0","id":id,"method":"usage.refresh","params":params});
        let Outcome::Reply(first) = core.dispatch(&connection, call(3)).unwrap() else {
            panic!("usage refresh did not reply")
        };
        let Outcome::Reply(repeated) = core.dispatch(&connection, call(4)).unwrap() else {
            panic!("repeated usage refresh did not reply")
        };
        assert_eq!(first["providers"][0]["primary"]["used"], 0.25);
        assert_eq!(repeated, first);
        let stored = core
            .store
            .usage_reading(&UsageProvider::Codex)
            .unwrap()
            .unwrap();
        assert_eq!(stored.reading.primary.unwrap().used, 0.25);
        let (_, events) = core.store.replay(0).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.method == "usage.changed")
                .count(),
            1
        );
        drop(core);
        let restored = Core::open(&directory).unwrap();
        assert_eq!(
            restored.usage_list().unwrap()["providers"][1]["state"],
            "stale"
        );
        drop(restored);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_refresh_is_published_without_replacing_the_last_durable_success() {
        let directory = std::env::temp_dir().join(format!("sn-engine-failed-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let mut core = Core::open(&directory).unwrap();
        core.clock = Arc::new(FixedClock(100_000));
        core.usage = service(vec![
            payload(40),
            SourceRead::NetworkError,
            SourceRead::Unavailable,
        ]);
        let (outbox, _receiver) = crate::outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"usage-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        core.usage_refresh(
            &connection,
            &json!({"provider":"codex","idempotencyKey":"successful"}),
        )
        .unwrap();
        core.clock = Arc::new(FixedClock(131_000));
        let failed = core
            .usage_refresh(
                &connection,
                &json!({"provider":"codex","idempotencyKey":"failed"}),
            )
            .unwrap();
        assert_eq!(failed["providers"][0]["state"], "unavailable");
        assert_eq!(
            core.usage_list().unwrap()["providers"][1]["state"],
            "unavailable"
        );
        let stored = core
            .store
            .usage_reading(&UsageProvider::Codex)
            .unwrap()
            .unwrap();
        assert_eq!(stored.reading.primary.unwrap().used, 0.4);
        assert_eq!(
            core.store
                .replay(0)
                .unwrap()
                .1
                .iter()
                .filter(|event| event.method == "usage.changed")
                .count(),
            2
        );
        drop(core);
        let restored = Core::open(&directory).unwrap();
        let reading = &restored.usage_list().unwrap()["providers"][1];
        assert_eq!(reading["state"], "stale");
        assert_eq!(reading["primary"]["used"], 0.4);
        drop(restored);
        fs::remove_dir_all(directory).unwrap();
    }
}
