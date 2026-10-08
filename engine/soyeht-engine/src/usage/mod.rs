//! Subscription usage: the readers' host, the background schedule, persistence and wire
//! publication. The engine is the only component that reads usage; clients only display it.

#[cfg(target_os = "macos")]
mod browser;
mod host;
mod process;
mod service;
mod terminal;
#[cfg(test)]
mod tests;

pub use host::SystemHost;
pub use service::{PreparedRefresh, Service};

use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use service::{Committed, ManualRefusal, RefreshBatch};
use soyeht_protocol::{
    UsageListResult, UsageReading, UsageRefreshParams, UsageRefreshResult, UsageWindow,
};
use soyeht_store::usage::StoredUsageReading;
use soyeht_usage::{Reading, Window};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// How often the background schedule looks for due providers.
const SCHEDULE_TICK: Duration = Duration::from_secs(15);

/// A manual `usage.refresh` whose sources are read after the engine lock is released.
pub struct PreparedUsageRefresh {
    connection: String,
    params: Value,
    refresh: PreparedRefresh,
}

impl PreparedUsageRefresh {
    pub fn read(self) -> (FinishUsageRefresh, RefreshBatch) {
        let batch = self.refresh.read();
        (
            FinishUsageRefresh {
                connection: self.connection,
                params: self.params,
            },
            batch,
        )
    }
}

pub struct FinishUsageRefresh {
    connection: String,
    params: Value,
}

impl Core {
    pub fn usage_list(&self) -> Result<Value> {
        let now_sec = nonnegative_time(self.clock.now_unix());
        serde_json::to_value(UsageListResult {
            providers: self
                .usage
                .providers()
                .iter()
                .map(|provider| wire_reading(&self.usage.shown(provider, now_sec)))
                .collect(),
        })
        .map_err(|_| errors::internal("Usage list serialization failed"))
    }

    /// `usage.refresh`: finishes outside the engine lock (see `deferred`).
    pub(crate) fn usage_refresh(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<crate::dispatch::Outcome> {
        self.usage_refresh_prepare(connection, params)
            .map(crate::dispatch::Outcome::from)
    }

    /// Chooses the providers of a click, or refuses it under the per-provider limit.
    pub(crate) fn usage_refresh_prepare(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<PreparedUsageRefresh> {
        let request: UsageRefreshParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid usage refresh parameters"))?;
        let now_sec = nonnegative_time(self.clock.now_unix());
        let timestamp = self.clock.timestamp();
        match self
            .usage
            .prepare_manual(request.provider, now_sec, &timestamp)
        {
            Ok(refresh) => Ok(PreparedUsageRefresh {
                connection: connection.to_owned(),
                params: params.clone(),
                refresh,
            }),
            Err(ManualRefusal::RateLimited { retry_after_sec }) => Err(errors::rate_limited(
                "Usage refresh is limited per provider",
                retry_after_sec,
            )),
        }
    }

    /// Commits a click's readings, stores the fresh ones and publishes every provider.
    pub(crate) fn usage_refresh_finish(
        &mut self,
        finish: FinishUsageRefresh,
        batch: RefreshBatch,
    ) -> Result<Value> {
        let now_sec = nonnegative_time(self.clock.now_unix());
        let committed = self.usage.commit(batch);
        let readings: Vec<UsageReading> = committed
            .iter()
            .map(|item| wire_reading(&item.shown.for_display(now_sec)))
            .collect();
        let result = serde_json::to_value(UsageRefreshResult {
            providers: readings.clone(),
        })
        .map_err(|_| errors::internal("Usage refresh serialization failed"))?;
        let events = change_events(&readings)?;
        let fresh = fresh_readings(&committed);
        let mutation = self.mutate(
            &finish.connection,
            "usage.refresh",
            &finish.params,
            |change| {
                for (reading, observed_at) in &fresh {
                    change.save_usage_reading(reading, *observed_at)?;
                }
                Ok((result, events))
            },
        )?;
        Ok(mutation.result)
    }

    /// The providers the background schedule reads now, if any.
    pub(crate) fn usage_automatic_prepare(&mut self) -> Option<PreparedRefresh> {
        let now_sec = nonnegative_time(self.clock.now_unix());
        let timestamp = self.clock.timestamp();
        self.usage.prepare_automatic(now_sec, &timestamp)
    }

    /// Commits a background refresh; only readings that changed are published.
    pub(crate) fn usage_automatic_commit(&mut self, batch: RefreshBatch) -> Result<()> {
        let now_sec = nonnegative_time(self.clock.now_unix());
        let committed = self.usage.commit(batch);
        let changed: Vec<UsageReading> = committed
            .iter()
            .filter(|item| item.changed || item.fresh)
            .map(|item| wire_reading(&item.shown.for_display(now_sec)))
            .collect();
        let fresh = fresh_readings(&committed);
        if changed.is_empty() && fresh.is_empty() {
            return Ok(());
        }
        let events = change_events(&changed)?;
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine",
            &uuid::Uuid::new_v4().to_string(),
            "usage.schedule",
            |change| {
                for (reading, observed_at) in &fresh {
                    change.save_usage_reading(reading, *observed_at)?;
                }
                Ok((json!({}), events))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }
}

/// Reads due providers every few seconds, without holding the engine lock while reading.
pub async fn schedule(core: Arc<Mutex<Core>>) -> Result<()> {
    let mut ticks = tokio::time::interval(SCHEDULE_TICK);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let prepared = {
            let mut core = core
                .lock()
                .map_err(|_| errors::internal("Engine state lock poisoned"))?;
            if !core.usage.schedules() {
                return Ok(());
            }
            core.usage_automatic_prepare()
        };
        let Some(prepared) = prepared else {
            continue;
        };
        let providers = prepared.providers().to_vec();
        let batch = tokio::task::spawn_blocking(move || prepared.read()).await;
        let mut core = core
            .lock()
            .map_err(|_| errors::internal("Engine state lock poisoned"))?;
        match batch {
            Ok(batch) => {
                if let Err(error) = core.usage_automatic_commit(batch) {
                    eprintln!(
                        "{}",
                        json!({"event": "engine.usage_schedule_error", "kind": errors::kind(&error)})
                    );
                }
            }
            Err(_) => core.usage.abandon(&providers),
        }
    }
}

fn change_events(readings: &[UsageReading]) -> Result<Vec<(String, Value)>> {
    readings
        .iter()
        .map(|reading| Ok(("usage.changed".into(), serde_json::to_value(reading)?)))
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(|_| errors::internal("Usage event serialization failed"))
}

/// Fresh successes to store, with the time they were read.
fn fresh_readings(committed: &[Committed]) -> Vec<(UsageReading, i64)> {
    committed
        .iter()
        .filter(|item| item.fresh)
        .map(|item| {
            (
                wire_reading(&item.shown),
                i64::try_from(item.shown.observed_at_sec).unwrap_or(i64::MAX),
            )
        })
        .collect()
}

fn nonnegative_time(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// The engine's usage service over the real host, restored from the stored readings.
pub fn system_service(stored: Vec<StoredUsageReading>) -> Service {
    let host = SystemHost::from_environment();
    let schedules = host.schedules();
    Service::restore(
        stored.into_iter().map(restored_reading).collect(),
        Arc::new(host),
        schedules,
    )
}

fn restored_reading(stored: StoredUsageReading) -> Reading {
    Reading {
        provider: stored.reading.provider,
        state: stored.reading.state,
        unavailable_reason: stored.reading.unavailable_reason,
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
        unavailable_reason: reading.unavailable_reason.clone(),
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
