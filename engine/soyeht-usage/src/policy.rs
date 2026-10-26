//! When the engine refreshes each provider on its own (CodexBar's refresh timer).
//!
//! Every provider is read at startup and then every five minutes, CodexBar's fixed default and
//! its Linux desktop default. After a network failure the next tries come sooner, on CodexBar's
//! connectivity ladder (15 s, 45 s, 2 min, 5 min). A provider whose window has just reset is
//! read again at once, as CodexBar's reset-boundary refresh does. Manual refreshes are limited
//! to one per provider every 30 seconds.

use crate::{Reading, TransientFailure};
use chrono::DateTime;
use soyeht_protocol::UsageProvider;

const MIN_INTERVAL: u64 = soyeht_protocol::limits::USAGE_REFRESH_MIN_INTERVAL_SEC as u64;
const AUTO_INTERVAL: u64 = soyeht_protocol::limits::USAGE_AUTO_REFRESH_INTERVAL_SEC as u64;
const NETWORK_RETRY_SEC: [u64; 4] = [15, 45, 120, 300];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualRefresh {
    Ready,
    RateLimited { retry_after_sec: u32 },
}

#[derive(Debug, Clone)]
struct Schedule {
    provider: UsageProvider,
    last_attempt_sec: Option<u64>,
    last_manual_sec: Option<u64>,
    network_failures: u32,
    /// A wait the owner of the schedule asks for on top of the interval (a signed-out account
    /// is asked again later and later). Zero means none.
    hold_sec: u64,
}

#[derive(Debug, Default)]
pub struct RefreshPolicy {
    schedules: Vec<Schedule>,
}

impl RefreshPolicy {
    /// Whether a click may refresh `provider`: one per provider every 30 seconds. Background
    /// refreshes do not count against it.
    pub fn manual(&self, provider: &UsageProvider, now_sec: u64) -> ManualRefresh {
        let Some(last) = self.find(provider).and_then(|s| s.last_manual_sec) else {
            return ManualRefresh::Ready;
        };
        let elapsed = now_sec.saturating_sub(last);
        if elapsed >= MIN_INTERVAL {
            ManualRefresh::Ready
        } else {
            ManualRefresh::RateLimited {
                retry_after_sec: u32::try_from(MIN_INTERVAL - elapsed).unwrap_or(u32::MAX),
            }
        }
    }

    /// Whether the background schedule should read `provider` now. `reading` is the reading
    /// currently shown, used for the reset boundary.
    pub fn automatic_due(
        &self,
        provider: &UsageProvider,
        now_sec: u64,
        reading: Option<&Reading>,
    ) -> bool {
        let Some(schedule) = self.find(provider) else {
            return true;
        };
        let Some(last) = schedule.last_attempt_sec else {
            return true;
        };
        let wait = match schedule.network_failures {
            0 => AUTO_INTERVAL.max(schedule.hold_sec),
            failures => {
                let step = usize::try_from(failures - 1).unwrap_or(usize::MAX);
                NETWORK_RETRY_SEC[step.min(NETWORK_RETRY_SEC.len() - 1)]
            }
        };
        now_sec.saturating_sub(last) >= wait
            || reading.is_some_and(|reading| reset_since(reading, last, now_sec))
    }

    pub fn record_manual(&mut self, provider: &UsageProvider, now_sec: u64) {
        self.schedule_mut(provider).last_manual_sec = Some(now_sec);
    }

    /// Records an attempt and whether it failed in a way that retries sooner.
    pub fn record(
        &mut self,
        provider: &UsageProvider,
        now_sec: u64,
        transient: Option<&TransientFailure>,
    ) {
        let schedule = self.schedule_mut(provider);
        schedule.last_attempt_sec = Some(now_sec);
        schedule.network_failures = match transient {
            Some(TransientFailure::Network) => schedule.network_failures.saturating_add(1),
            Some(TransientFailure::RateLimited { .. }) | None => 0,
        };
    }

    /// Makes the background schedule wait at least `seconds` after each attempt at `provider`,
    /// until it is called again; `0` clears it.
    pub fn hold(&mut self, provider: &UsageProvider, seconds: u64) {
        self.schedule_mut(provider).hold_sec = seconds;
    }

    fn find(&self, provider: &UsageProvider) -> Option<&Schedule> {
        self.schedules
            .iter()
            .find(|schedule| &schedule.provider == provider)
    }

    fn schedule_mut(&mut self, provider: &UsageProvider) -> &mut Schedule {
        let index = match self
            .schedules
            .iter()
            .position(|schedule| &schedule.provider == provider)
        {
            Some(index) => index,
            None => {
                self.schedules.push(Schedule {
                    provider: provider.clone(),
                    last_attempt_sec: None,
                    last_manual_sec: None,
                    network_failures: 0,
                    hold_sec: 0,
                });
                self.schedules.len() - 1
            }
        };
        &mut self.schedules[index]
    }
}

/// A window of the shown reading reset after the last attempt and before now.
fn reset_since(reading: &Reading, last_attempt_sec: u64, now_sec: u64) -> bool {
    [reading.primary.as_ref(), reading.secondary.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|window| DateTime::parse_from_rfc3339(&window.resets_at).ok())
        .filter_map(|reset| u64::try_from(reset.timestamp()).ok())
        .any(|reset| reset > last_attempt_sec && reset <= now_sec)
}
