//! Exponential retry backoff with a bounded log rate.
//!
//! One streak of consecutive failures of one thing that is retried: an editor
//! autosave, a disk watch, a connection. The caller keeps one [`Backoff`] per
//! streak and does three things with it:
//!
//! 1. after each failed attempt, call [`Backoff::fail`], wait
//!    [`Backoff::next_delay_ms`] before trying again, and log the failure only
//!    when [`Backoff::should_log`] is true;
//! 2. after a successful attempt, call [`Backoff::succeed`], which ends the
//!    streak and says how many attempts it took (log that once, if it is
//!    `Some`).
//!
//! The delay doubles with every failure from `base_ms` up to `cap_ms`. Logging
//! happens at attempts 1, 2, 4, 8, ..., so a failure that lasts for days writes
//! a few lines instead of one per retry.

/// Failures counted before the delay stops doubling; keeps the shift in range.
const MAX_DOUBLINGS: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    base_ms: i64,
    cap_ms: i64,
    attempts: u32,
}

impl Backoff {
    /// `base_ms` is the normal interval of the retried thing; the delay after
    /// the first failure is twice that. `cap_ms` is the longest delay; a cap
    /// below `base_ms` is raised to it.
    pub const fn new(base_ms: i64, cap_ms: i64) -> Self {
        let cap_ms = if cap_ms < base_ms { base_ms } else { cap_ms };
        Self {
            base_ms,
            cap_ms,
            attempts: 0,
        }
    }

    /// Counts one more consecutive failure.
    pub fn fail(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
    }

    /// Ends the streak. Returns the number of failed attempts it had, or
    /// `None` when there was no failure.
    pub fn succeed(&mut self) -> Option<u32> {
        let attempts = std::mem::take(&mut self.attempts);
        (attempts > 0).then_some(attempts)
    }

    /// Milliseconds to wait before the next attempt: `base_ms` doubled once
    /// per counted failure, never above `cap_ms`.
    pub fn next_delay_ms(&self) -> i64 {
        self.base_ms
            .saturating_mul(1_i64 << self.attempts.min(MAX_DOUBLINGS))
            .clamp(self.base_ms, self.cap_ms)
    }

    /// Whether the failure just counted should be logged: attempts 1, 2, 4, 8
    /// and so on.
    pub fn should_log(&self) -> bool {
        self.attempts.is_power_of_two()
    }

    /// Consecutive failures counted so far.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delay_doubles_up_to_the_cap() {
        let mut backoff = Backoff::new(250, 60_000);
        assert_eq!(backoff.next_delay_ms(), 250);
        let delays: Vec<i64> = (0..12)
            .map(|_| {
                backoff.fail();
                backoff.next_delay_ms()
            })
            .collect();
        assert_eq!(&delays[..4], &[500, 1_000, 2_000, 4_000]);
        assert!(delays.iter().all(|delay| *delay <= 60_000));
        assert_eq!(delays.last(), Some(&60_000));
    }

    #[test]
    fn a_cap_below_the_base_is_raised_to_it() {
        let mut backoff = Backoff::new(1_000, 10);
        backoff.fail();
        assert_eq!(backoff.next_delay_ms(), 1_000);
    }

    #[test]
    fn a_long_streak_logs_only_at_powers_of_two() {
        let mut backoff = Backoff::new(250, 60_000);
        let logged = (0..10_000)
            .filter(|_| {
                backoff.fail();
                backoff.should_log()
            })
            .count();
        assert_eq!(logged, 14);
    }

    #[test]
    fn success_ends_the_streak_and_reports_its_length() {
        let mut backoff = Backoff::new(250, 60_000);
        assert_eq!(backoff.succeed(), None);
        backoff.fail();
        backoff.fail();
        assert_eq!(backoff.succeed(), Some(2));
        assert_eq!(backoff.attempts(), 0);
        assert_eq!(backoff.next_delay_ms(), 250);
    }
}
