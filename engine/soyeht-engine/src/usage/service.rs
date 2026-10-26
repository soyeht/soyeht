//! What each usage ring shows, and which refreshes run next.
//!
//! Reading happens outside the engine lock: [`Service::prepare_manual`] and
//! [`Service::prepare_automatic`] hand out a [`PreparedRefresh`] that walks the source chains on
//! a worker thread, and [`Service::commit`] folds its outcomes back in. A provider is read by at
//! most one refresh at a time.
//!
//! After a failure the last good reading stays, marked last known, with the failure's reason
//! (CodexBar keeps the previous snapshot). The first failure after a current reading is not
//! shown at all, as CodexBar's consecutive-failure gate hides it.

use crate::streak::Streak;
use soyeht_protocol::{
    UsageProvider, UsageSource, UsageState, UsageUnavailableReason,
    limits::{USAGE_AUTO_REFRESH_INTERVAL_SEC, USAGE_MAX_BACKOFF_SEC},
};
use soyeht_usage::{
    ManualRefresh, Planner, Reading, RefreshOutcome, RefreshPolicy, Trigger, host::Host,
};
use std::sync::Arc;

pub struct Service {
    planner: Arc<Planner>,
    policy: RefreshPolicy,
    readings: Vec<Reading>,
    failure_streaks: Vec<(UsageProvider, u32)>,
    signed_out: Vec<(UsageProvider, Streak)>,
    in_flight: Vec<UsageProvider>,
    schedules: bool,
}

/// Refreshes chosen under the engine lock, to be read without it.
pub struct PreparedRefresh {
    planner: Arc<Planner>,
    providers: Vec<UsageProvider>,
    trigger: Trigger,
    now_sec: u64,
    fetched_at: String,
}

#[derive(Debug)]
pub struct RefreshBatch {
    now_sec: u64,
    /// A click read it: a click is neither counted in the signed-out streak nor held back.
    manual: bool,
    outcomes: Vec<RefreshOutcome>,
}

/// One provider after a commit: what the ring shows now, and whether it is a fresh success to
/// store.
#[derive(Debug, Clone, PartialEq)]
pub struct Committed {
    pub shown: Reading,
    pub fresh: bool,
    pub changed: bool,
    /// This answer is to be logged as a signed-out provider: the first one of a streak, and
    /// then each time the count of reads reaches a power of two.
    pub log_signed_out: bool,
}

pub enum ManualRefusal {
    RateLimited { retry_after_sec: u32 },
}

/// A manual refresh that meets a background refresh of the same provider waits this long.
const IN_FLIGHT_RETRY_SEC: u32 = 5;

impl PreparedRefresh {
    pub fn providers(&self) -> &[UsageProvider] {
        &self.providers
    }

    /// Reads every provider in parallel; each chain bounds its own waits.
    pub fn read(self) -> RefreshBatch {
        let outcomes = std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .providers
                .iter()
                .map(|provider| {
                    let planner = &self.planner;
                    let fetched_at = &self.fetched_at;
                    let (trigger, now_sec) = (self.trigger, self.now_sec);
                    scope.spawn(move || planner.refresh(provider, trigger, now_sec, fetched_at))
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });
        RefreshBatch {
            now_sec: self.now_sec,
            manual: self.trigger == Trigger::Manual,
            outcomes,
        }
    }
}

impl Service {
    /// `schedules` is false only for the end-to-end suite, which drives every refresh itself.
    pub fn restore(stored: Vec<Reading>, host: Arc<dyn Host>, schedules: bool) -> Self {
        Self {
            planner: Arc::new(Planner::new(host)),
            policy: RefreshPolicy::default(),
            readings: stored.iter().map(Reading::restored_from_store).collect(),
            failure_streaks: Vec::new(),
            signed_out: Vec::new(),
            in_flight: Vec::new(),
            schedules,
        }
    }

    pub fn schedules(&self) -> bool {
        self.schedules
    }

    pub fn providers(&self) -> Vec<UsageProvider> {
        self.planner.providers()
    }

    pub fn shown(&self, provider: &UsageProvider, now_sec: u64) -> Reading {
        self.reading(provider)
            .map(|reading| reading.for_display(now_sec))
            .unwrap_or_else(|| self.not_checked(provider, now_sec))
    }

    fn reading(&self, provider: &UsageProvider) -> Option<&Reading> {
        self.readings
            .iter()
            .find(|reading| &reading.provider == provider)
    }

    fn not_checked(&self, provider: &UsageProvider, now_sec: u64) -> Reading {
        Reading {
            provider: provider.clone(),
            state: UsageState::Unavailable,
            unavailable_reason: Some(UsageUnavailableReason::NotChecked),
            primary: None,
            secondary: None,
            source: self
                .planner
                .sources(provider)
                .into_iter()
                .next()
                .unwrap_or(UsageSource::ClaudeOauthEndpoint),
            fetched_at: String::new(),
            observed_at_sec: now_sec,
        }
    }

    /// A click: every chosen provider, or none when one of them is inside its 30-second limit
    /// or already being read (all or none, ADR 0044).
    pub fn prepare_manual(
        &mut self,
        provider: Option<UsageProvider>,
        now_sec: u64,
        fetched_at: &str,
    ) -> Result<PreparedRefresh, ManualRefusal> {
        let providers = provider.map_or_else(|| self.providers(), |provider| vec![provider]);
        let mut wait = providers
            .iter()
            .filter_map(|provider| match self.policy.manual(provider, now_sec) {
                ManualRefresh::Ready => None,
                ManualRefresh::RateLimited { retry_after_sec } => Some(retry_after_sec),
            })
            .max();
        if providers
            .iter()
            .any(|provider| self.in_flight.contains(provider))
        {
            wait = Some(wait.unwrap_or(0).max(IN_FLIGHT_RETRY_SEC));
        }
        if let Some(retry_after_sec) = wait {
            return Err(ManualRefusal::RateLimited { retry_after_sec });
        }
        for provider in &providers {
            self.policy.record_manual(provider, now_sec);
        }
        Ok(self.prepare(providers, Trigger::Manual, now_sec, fetched_at))
    }

    /// The providers the background schedule wants now, if any.
    pub fn prepare_automatic(&mut self, now_sec: u64, fetched_at: &str) -> Option<PreparedRefresh> {
        let due: Vec<UsageProvider> = self
            .providers()
            .into_iter()
            .filter(|provider| !self.in_flight.contains(provider))
            .filter(|provider| {
                self.policy
                    .automatic_due(provider, now_sec, self.reading(provider))
            })
            .collect();
        (!due.is_empty()).then(|| self.prepare(due, Trigger::Automatic, now_sec, fetched_at))
    }

    fn prepare(
        &mut self,
        providers: Vec<UsageProvider>,
        trigger: Trigger,
        now_sec: u64,
        fetched_at: &str,
    ) -> PreparedRefresh {
        self.in_flight.extend(providers.iter().cloned());
        PreparedRefresh {
            planner: Arc::clone(&self.planner),
            providers,
            trigger,
            now_sec,
            fetched_at: fetched_at.to_owned(),
        }
    }

    /// Releases providers whose refresh never produced a batch.
    pub fn abandon(&mut self, providers: &[UsageProvider]) {
        self.in_flight
            .retain(|provider| !providers.contains(provider));
    }

    pub fn commit(&mut self, batch: RefreshBatch) -> Vec<Committed> {
        let mut committed = Vec::with_capacity(batch.outcomes.len());
        for outcome in batch.outcomes {
            let provider = outcome.reading.provider.clone();
            self.in_flight.retain(|item| item != &provider);
            self.policy
                .record(&provider, batch.now_sec, outcome.transient.as_ref());
            let log_signed_out = self.track_signed_out(&provider, &outcome.reading, batch.manual);
            let previous = self.reading(&provider).cloned();
            let fresh = outcome.reading.state == UsageState::Ok;
            let shown = if fresh {
                self.set_streak(&provider, 0);
                outcome.reading
            } else {
                let streak = self.bump_streak(&provider);
                kept_or_failed(previous.as_ref(), outcome.reading, streak, batch.now_sec)
            };
            let changed = previous.as_ref() != Some(&shown);
            replace(&mut self.readings, shown.clone());
            committed.push(Committed {
                shown,
                fresh,
                changed,
                log_signed_out,
            });
        }
        committed
    }

    /// A signed-out provider is read again at doubling intervals, from the background schedule's
    /// five minutes up to the longest backoff; any other answer ends the streak. A click is not
    /// held back, and a click that finds it still signed out leaves the streak as it is.
    /// Returns whether this answer is to be logged.
    fn track_signed_out(
        &mut self,
        provider: &UsageProvider,
        reading: &Reading,
        manual: bool,
    ) -> bool {
        let position = match self.signed_out.iter().position(|(p, _)| p == provider) {
            Some(position) => position,
            None => {
                self.signed_out.push((
                    provider.clone(),
                    Streak::retrying(
                        (USAGE_AUTO_REFRESH_INTERVAL_SEC as i64) * 1_000,
                        (USAGE_MAX_BACKOFF_SEC as i64) * 1_000,
                    ),
                ));
                self.signed_out.len() - 1
            }
        };
        let streak = &mut self.signed_out[position].1;
        let signed_out = reading.state == UsageState::LoginNeeded;
        if signed_out && manual {
            return false;
        }
        let log = signed_out && streak.fail("login_needed");
        if signed_out {
            let hold = u64::try_from(streak.next_delay_ms() / 1_000).unwrap_or(0);
            self.policy.hold(provider, hold);
        } else {
            streak.succeed();
            self.policy.hold(provider, 0);
        }
        log
    }

    fn set_streak(&mut self, provider: &UsageProvider, value: u32) {
        match self.failure_streaks.iter_mut().find(|(p, _)| p == provider) {
            Some((_, streak)) => *streak = value,
            None => self.failure_streaks.push((provider.clone(), value)),
        }
    }

    fn bump_streak(&mut self, provider: &UsageProvider) -> u32 {
        let next = self
            .failure_streaks
            .iter()
            .find(|(p, _)| p == provider)
            .map_or(1, |(_, streak)| streak.saturating_add(1));
        self.set_streak(provider, next);
        next
    }
}

/// The failure's reading, or the last good one kept as last known. A current reading survives
/// its first failure unchanged.
fn kept_or_failed(
    previous: Option<&Reading>,
    failure: Reading,
    streak: u32,
    now_sec: u64,
) -> Reading {
    match previous.filter(|reading| reading.has_windows()) {
        Some(previous) if streak == 1 && previous.for_display(now_sec).state == UsageState::Ok => {
            previous.clone()
        }
        Some(previous) => previous.kept_after_failure(failure.unavailable_reason),
        None => failure,
    }
}

fn replace(readings: &mut Vec<Reading>, replacement: Reading) {
    match readings
        .iter_mut()
        .find(|reading| reading.provider == replacement.provider)
    {
        Some(reading) => *reading = replacement,
        None => readings.push(replacement),
    }
}
