//! One streak of consecutive failures of one thing that is retried or polled, with the
//! backoff between attempts and the rule for logging it.
//!
//! Every place that logs a failure that can repeat keeps one [`Streak`] per thing: an
//! editor autosave, a disk watch, a tick, a background job, a schedule. After each
//! failed attempt it calls [`Streak::fail`] with the failure's kind and logs only when
//! that returns true. After a successful attempt it calls [`Streak::succeed`], which ends
//! the streak and says how many attempts it took (log that once, if it is `Some`).
//!
//! A streak logs at its first failure and whenever the kind of the failure changes. A
//! [`Streak::retrying`] streak also logs at attempts 2, 4, 8, ..., so a failure that lasts
//! for days writes a few lines instead of one per retry, and it waits twice as long after
//! each failure ([`Streak::next_delay_ms`]) up to a cap. A [`Streak::once`] streak logs no
//! more than that, and has no delay.

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};

/// Failures counted before the delay stops doubling; keeps the shift in range.
const MAX_DOUBLINGS: u32 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Streak {
    base_ms: i64,
    cap_ms: i64,
    repeat: bool,
    attempts: u32,
    kind: Option<String>,
}

impl Streak {
    /// `base_ms` is the normal interval of the retried thing; the delay after the first
    /// failure is twice that. `cap_ms` is the longest delay; a cap below `base_ms` is raised
    /// to it. The streak logs again at attempts 2, 4, 8 and so on.
    pub const fn retrying(base_ms: i64, cap_ms: i64) -> Self {
        let cap_ms = if cap_ms < base_ms { base_ms } else { cap_ms };
        Self {
            base_ms,
            cap_ms,
            repeat: true,
            attempts: 0,
            kind: None,
        }
    }

    /// A streak that only counts and says when to log: the first failure and each change
    /// of kind.
    pub const fn once() -> Self {
        Self {
            base_ms: 0,
            cap_ms: 0,
            repeat: false,
            attempts: 0,
            kind: None,
        }
    }

    /// Counts one more consecutive failure of `kind`. Returns whether to log it.
    pub fn fail(&mut self, kind: &str) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        let changed = self.kind.as_deref() != Some(kind);
        if changed {
            self.kind = Some(kind.to_owned());
        }
        changed || (self.repeat && self.attempts.is_power_of_two())
    }

    /// Ends the streak. Returns the number of failed attempts it had, or `None` when there
    /// was no failure.
    pub fn succeed(&mut self) -> Option<u32> {
        self.kind = None;
        let attempts = std::mem::take(&mut self.attempts);
        (attempts > 0).then_some(attempts)
    }

    /// Milliseconds to wait before the next attempt: `base_ms` doubled once per counted
    /// failure, never above `cap_ms`.
    pub fn next_delay_ms(&self) -> i64 {
        self.base_ms
            .saturating_mul(1_i64 << self.attempts.min(MAX_DOUBLINGS))
            .clamp(self.base_ms, self.cap_ms)
    }

    /// Consecutive failures counted so far.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

/// Counts a failure of `kind` in a streak kept in a static. True when it is to be logged: the
/// first of a streak and each change of kind.
pub fn first_failure(streak: &Mutex<Streak>, kind: &str) -> bool {
    streak
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .fail(kind)
}

/// Ends a static streak after an attempt that worked.
pub fn recovered(streak: &Mutex<Streak>) {
    streak
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .succeed();
}

/// One [`Streak::once`] per name, for a failure that repeats for each of several things (the
/// harnesses of a catalogue), so one that recovers does not reset another that keeps failing.
#[derive(Default)]
pub struct KeyedStreaks(Mutex<BTreeMap<String, Streak>>);

impl KeyedStreaks {
    pub const fn new() -> Self {
        Self(Mutex::new(BTreeMap::new()))
    }

    /// Counts a failure of `kind` for `key`. True when it is to be logged.
    pub fn fail(&self, key: &str, kind: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.to_owned())
            .or_insert_with(Streak::once)
            .fail(kind)
    }

    /// Ends the streak of `key` after an attempt that worked.
    pub fn succeed(&self, key: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_static_streak_logs_a_repeated_failure_once_and_again_after_a_recovery() {
        let streak = Mutex::new(Streak::once());
        let logged: Vec<bool> = (0..4).map(|_| first_failure(&streak, "stage")).collect();
        assert_eq!(logged, [true, false, false, false]);
        recovered(&streak);
        assert!(first_failure(&streak, "stage"));
    }

    #[test]
    fn keyed_streaks_do_not_reset_each_other() {
        let streaks = KeyedStreaks::new();
        assert!(streaks.fail("a", "io"));
        assert!(streaks.fail("b", "io"));
        streaks.succeed("b");
        assert!(
            !streaks.fail("a", "io"),
            "a keeps its streak while b recovers"
        );
        assert!(streaks.fail("b", "io"), "b starts a new streak");
    }

    #[test]
    fn the_delay_doubles_up_to_the_cap() {
        let mut streak = Streak::retrying(250, 60_000);
        assert_eq!(streak.next_delay_ms(), 250);
        let delays: Vec<i64> = (0..12)
            .map(|_| {
                streak.fail("io");
                streak.next_delay_ms()
            })
            .collect();
        assert_eq!(&delays[..4], &[500, 1_000, 2_000, 4_000]);
        assert!(delays.iter().all(|delay| *delay <= 60_000));
        assert_eq!(delays.last(), Some(&60_000));
    }

    #[test]
    fn a_cap_below_the_base_is_raised_to_it() {
        let mut streak = Streak::retrying(1_000, 10);
        streak.fail("io");
        assert_eq!(streak.next_delay_ms(), 1_000);
    }

    #[test]
    fn a_long_retrying_streak_logs_only_at_powers_of_two() {
        let mut streak = Streak::retrying(250, 60_000);
        let logged = (0..10_000).filter(|_| streak.fail("io")).count();
        assert_eq!(logged, 14);
    }

    #[test]
    fn a_once_streak_logs_its_first_failure_and_nothing_more_of_the_same_kind() {
        let mut streak = Streak::once();
        let logged: Vec<bool> = (0..6).map(|_| streak.fail("storage")).collect();
        assert_eq!(logged, [true, false, false, false, false, false]);
        assert_eq!(streak.attempts(), 6);
        assert_eq!(streak.next_delay_ms(), 0);
    }

    #[test]
    fn a_change_of_kind_logs_again_in_either_kind_of_streak() {
        for mut streak in [Streak::once(), Streak::retrying(250, 60_000)] {
            assert!(streak.fail("storage"));
            assert!(streak.fail("supervisor"));
            assert!(streak.fail("storage"));
        }
    }

    #[test]
    fn success_ends_the_streak_and_reports_its_length() {
        let mut streak = Streak::retrying(250, 60_000);
        assert_eq!(streak.succeed(), None);
        streak.fail("io");
        streak.fail("io");
        assert_eq!(streak.succeed(), Some(2));
        assert_eq!(streak.attempts(), 0);
        assert_eq!(streak.next_delay_ms(), 250);
    }

    #[test]
    fn a_failure_after_a_recovery_logs_again() {
        let mut streak = Streak::once();
        assert!(streak.fail("io"));
        assert!(!streak.fail("io"));
        assert_eq!(streak.succeed(), Some(2));
        assert!(streak.fail("io"));
    }
}
