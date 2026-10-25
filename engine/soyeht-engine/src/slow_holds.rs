//! The rule for logging a Core lock hold that took too long (ADR 0079, AGENTS.md "Logging").
//!
//! A hold that repeats is one condition, not one event per hold: a loaded machine can hold the
//! lock past the threshold every second, and a line each time buries every other event of the
//! log. The rule is the one of [`crate::streak::Streak`], kept per purpose (`tick`, `dispatch`,
//! ...): the first slow hold of a run is logged at once (`engine.core_lock_slow`), so is a hold
//! at least twice as long as the longest one logged so far in the run, and every
//! [`SUMMARY_EVERY_MS`] the holds written off since the last line are summed up in one line
//! (`engine.core_lock_slow_summary`: how many, the worst, and the window). A run ends after a whole
//! window with no slow hold, and the next one logs its first hold again.

use soyeht_platform::log::Line;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock, PoisonError},
    time::Instant,
};

/// How long a run goes between its summaries.
pub(crate) const SUMMARY_EVERY_MS: u64 = 60_000;

struct Run {
    /// The longest hold that was logged on its own in this run.
    logged_worst_ms: u64,
    /// Holds counted since the last line of the run, and the longest of them.
    counted: u32,
    counted_worst_ms: u64,
    /// When the window of the next summary opened, and when the last slow hold ended.
    window_start_ms: u64,
    last_hold_ms: u64,
}

/// The slow holds of every purpose. Pure: the caller says what time it is.
#[derive(Default)]
pub(crate) struct SlowHolds {
    runs: BTreeMap<&'static str, Run>,
}

impl SlowHolds {
    /// Counts one hold of `held_ms` for `purpose` that ended at `now_ms`. Returns the line to
    /// write now, if this hold is the first of its run or much longer than the ones logged.
    pub(crate) fn record(
        &mut self,
        purpose: &'static str,
        held_ms: u64,
        now_ms: u64,
    ) -> Option<Line> {
        let Some(run) = self.runs.get_mut(purpose) else {
            self.runs.insert(
                purpose,
                Run {
                    logged_worst_ms: held_ms,
                    counted: 0,
                    counted_worst_ms: 0,
                    window_start_ms: now_ms,
                    last_hold_ms: now_ms,
                },
            );
            return Some(hold_line(purpose, held_ms));
        };
        run.last_hold_ms = now_ms;
        if held_ms >= run.logged_worst_ms.saturating_mul(2) {
            run.logged_worst_ms = held_ms;
            return Some(hold_line(purpose, held_ms));
        }
        run.counted = run.counted.saturating_add(1);
        run.counted_worst_ms = run.counted_worst_ms.max(held_ms);
        None
    }

    /// The summaries whose window is over at `now_ms`. A run with no slow hold for a whole
    /// window ends here.
    pub(crate) fn due(&mut self, now_ms: u64) -> Vec<Line> {
        let mut lines = Vec::new();
        self.runs.retain(|purpose, run| {
            let window_ms = now_ms.saturating_sub(run.window_start_ms);
            if window_ms < SUMMARY_EVERY_MS {
                return true;
            }
            if run.counted > 0 {
                lines.push(
                    Line::new("engine.core_lock_slow_summary")
                        .field("purpose", purpose)
                        .field("count", run.counted)
                        .field("worstMs", run.counted_worst_ms)
                        .field("windowMs", window_ms),
                );
            }
            run.counted = 0;
            run.counted_worst_ms = 0;
            run.window_start_ms = now_ms;
            now_ms.saturating_sub(run.last_hold_ms) < SUMMARY_EVERY_MS
        });
        lines
    }
}

fn hold_line(purpose: &str, held_ms: u64) -> Line {
    Line::new("engine.core_lock_slow")
        .field("purpose", purpose)
        .field("heldMs", held_ms)
}

static HOLDS: Mutex<Option<SlowHolds>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Counts a slow hold of the engine's lock and writes the line the rule asks for.
pub(crate) fn note(purpose: &'static str, held_ms: u64) {
    let line = HOLDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(SlowHolds::default)
        .record(purpose, held_ms, now_ms());
    if let Some(line) = line {
        line.emit();
    }
}

/// Writes the summaries whose window is over. The tick calls it, outside the lock.
pub(crate) fn emit_due() {
    let lines = HOLDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .map(|holds| holds.due(now_ms()))
        .unwrap_or_default();
    for line in lines {
        line.emit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(line: Option<Line>) -> String {
        line.map(|line| line.json()).unwrap_or_default()
    }

    #[test]
    fn a_run_logs_its_first_hold_and_counts_the_ones_like_it() {
        let mut holds = SlowHolds::default();
        assert_eq!(
            json(holds.record("tick", 120, 0)),
            r#"{"event":"engine.core_lock_slow","purpose":"tick","heldMs":"120"}"#
        );
        // Not the first and not twice as long as the 120 ms already logged.
        let logged = (1..=2_000)
            .filter_map(|n| holds.record("tick", 100 + n % 100, n * 20))
            .count();
        assert_eq!(logged, 0);
    }

    #[test]
    fn a_hold_twice_as_long_as_the_longest_logged_is_logged_at_once() {
        let mut holds = SlowHolds::default();
        assert!(holds.record("tick", 120, 0).is_some());
        assert!(holds.record("tick", 239, 10).is_none());
        assert!(holds.record("tick", 240, 20).is_some());
        assert!(holds.record("tick", 400, 30).is_none());
        assert!(holds.record("tick", 480, 40).is_some());
    }

    #[test]
    fn each_purpose_has_its_own_run() {
        let mut holds = SlowHolds::default();
        assert!(holds.record("tick", 120, 0).is_some());
        assert!(holds.record("dispatch", 120, 1).is_some());
        assert!(holds.record("tick", 120, 2).is_none());
        assert!(holds.record("dispatch", 120, 3).is_none());
    }

    #[test]
    fn a_window_ends_in_one_summary_with_the_count_and_the_worst() {
        let mut holds = SlowHolds::default();
        assert!(holds.record("tick", 150, 0).is_some());
        for (n, held) in [110, 190, 130, 105].into_iter().enumerate() {
            assert!(holds.record("tick", held, 1_000 * (n as u64 + 1)).is_none());
        }
        assert!(holds.due(SUMMARY_EVERY_MS - 1).is_empty());
        let lines = holds.due(SUMMARY_EVERY_MS);
        assert_eq!(lines.len(), 1);
        let line = lines[0].json();
        assert!(line.contains("engine.core_lock_slow_summary"), "{line}");
        assert!(line.contains(r#""count":"4""#), "{line}");
        assert!(line.contains(r#""worstMs":"190""#), "{line}");
        assert!(line.contains(r#""windowMs":"60000""#), "{line}");
        assert!(line.contains(r#""purpose":"tick""#), "{line}");
    }

    #[test]
    fn a_quiet_window_writes_no_summary_and_ends_the_run() {
        let mut holds = SlowHolds::default();
        assert!(holds.record("tick", 150, 0).is_some());
        // Nothing was counted besides the logged first hold: nothing to sum up.
        assert!(holds.due(SUMMARY_EVERY_MS).is_empty());
        assert!(holds.due(2 * SUMMARY_EVERY_MS).is_empty());
        // The run is over: the next slow hold is a first one again.
        assert!(
            holds
                .record("tick", 110, 2 * SUMMARY_EVERY_MS + 1)
                .is_some()
        );
    }

    #[test]
    fn a_run_that_keeps_going_keeps_its_summaries_and_never_logs_every_hold() {
        let mut holds = SlowHolds::default();
        let mut written = 0;
        // 40 minutes of a hold every 1.2 s, as on the loaded VM that wrote 2068 lines.
        for n in 0..2_000_u64 {
            let now = n * 1_200;
            written += usize::from(holds.record("tick", 115 + n % 200, now).is_some());
            written += holds.due(now).len();
        }
        assert!(written <= 2 + 40, "{written} lines for 2000 holds");
        assert!(written >= 38, "a summary per minute: {written}");
    }
}
