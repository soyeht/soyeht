use crate::TransientFailure;
use soyeht_protocol::UsageProvider;

const MIN_INTERVAL: u64 = soyeht_protocol::limits::USAGE_REFRESH_MIN_INTERVAL_SEC as u64;
const HOVER_AGE: u64 = soyeht_protocol::limits::USAGE_HOVER_REFRESH_AFTER_SEC as u64;
const OPEN_INTERVAL: u64 = soyeht_protocol::limits::USAGE_OPEN_REFRESH_INTERVAL_SEC as u64;
const MAX_BACKOFF: u64 = soyeht_protocol::limits::USAGE_MAX_BACKOFF_SEC as u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomaticTrigger {
    RailOpen,
    Hover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualRefresh {
    Ready,
    RateLimited { retry_after_sec: u32 },
}

#[derive(Debug, Clone)]
struct Schedule {
    provider: UsageProvider,
    last_attempt_sec: Option<u64>,
    transient_failures: u32,
    provider_retry_after_sec: u64,
}

#[derive(Debug, Default)]
pub struct RefreshPolicy {
    schedules: Vec<Schedule>,
}

impl RefreshPolicy {
    pub fn manual(&self, provider: UsageProvider, now_sec: u64) -> ManualRefresh {
        let Some(schedule) = self.find(provider) else {
            return ManualRefresh::Ready;
        };
        let elapsed = schedule
            .last_attempt_sec
            .map_or(MIN_INTERVAL, |last| now_sec.saturating_sub(last));
        if elapsed >= MIN_INTERVAL {
            ManualRefresh::Ready
        } else {
            ManualRefresh::RateLimited {
                retry_after_sec: u32::try_from(MIN_INTERVAL - elapsed).unwrap_or(u32::MAX),
            }
        }
    }

    pub fn automatic_due(
        &self,
        provider: UsageProvider,
        trigger: AutomaticTrigger,
        now_sec: u64,
        reading_age_sec: Option<u64>,
    ) -> bool {
        if trigger == AutomaticTrigger::Hover && reading_age_sec.is_some_and(|age| age <= HOVER_AGE)
        {
            return false;
        }
        let Some(schedule) = self.find(provider) else {
            return true;
        };
        let base = match trigger {
            AutomaticTrigger::RailOpen => OPEN_INTERVAL,
            AutomaticTrigger::Hover => MIN_INTERVAL,
        };
        let wait = base
            .max(backoff(schedule.transient_failures))
            .max(schedule.provider_retry_after_sec);
        schedule
            .last_attempt_sec
            .is_none_or(|last| now_sec.saturating_sub(last) >= wait)
    }

    pub fn record(
        &mut self,
        provider: UsageProvider,
        now_sec: u64,
        transient: Option<&TransientFailure>,
    ) {
        let schedule = self.schedule_mut(provider);
        schedule.last_attempt_sec = Some(now_sec);
        match transient {
            Some(TransientFailure::Network) => {
                schedule.transient_failures = schedule.transient_failures.saturating_add(1);
                schedule.provider_retry_after_sec = 0;
            }
            Some(TransientFailure::RateLimited { retry_after_sec }) => {
                schedule.transient_failures = schedule.transient_failures.saturating_add(1);
                schedule.provider_retry_after_sec = u64::from(*retry_after_sec).min(MAX_BACKOFF);
            }
            None => {
                schedule.transient_failures = 0;
                schedule.provider_retry_after_sec = 0;
            }
        }
    }

    fn find(&self, provider: UsageProvider) -> Option<&Schedule> {
        self.schedules
            .iter()
            .find(|schedule| schedule.provider == provider)
    }

    fn schedule_mut(&mut self, provider: UsageProvider) -> &mut Schedule {
        if let Some(index) = self
            .schedules
            .iter()
            .position(|schedule| schedule.provider == provider)
        {
            return &mut self.schedules[index];
        }
        self.schedules.push(Schedule {
            provider,
            last_attempt_sec: None,
            transient_failures: 0,
            provider_retry_after_sec: 0,
        });
        let last = self.schedules.len() - 1;
        &mut self.schedules[last]
    }
}

fn backoff(failures: u32) -> u64 {
    if failures == 0 {
        return 0;
    }
    let exponent = failures.saturating_sub(1).min(3);
    OPEN_INTERVAL
        .saturating_mul(1_u64 << exponent)
        .min(MAX_BACKOFF)
}
