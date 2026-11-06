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
//!
//! A hold is slow when it was held, or its caller waited for the lock, for [`SLOW`] or more, and the
//! line says which and where the time went (statements, the commit, segment files, the file port).
//! Every hold, slow or not, also goes to the per-minute aggregate of [`crate::lock_usage`].

use crate::lock_usage::{Observation, Usage, millis};
use soyeht_platform::log::Line;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock, PoisonError},
    time::Instant,
};

/// How long a run goes between its summaries.
pub(crate) const SUMMARY_EVERY_MS: u64 = 60_000;

/// A hold, or a wait for the lock, this long delays terminal output by more than a frame or two.
pub(crate) const SLOW: std::time::Duration = std::time::Duration::from_millis(100);

struct Run {
    /// The longest hold, or wait, that was logged on its own in this run.
    logged_worst_ms: u64,
    /// Holds counted since the last line of the run, and the longest of them.
    counted: u32,
    counted_worst_ms: u64,
    counted_worst_wait_ms: u64,
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
    /// Counts one slow hold that ended at `now_ms`. Returns the line to write now, if this hold is
    /// the first of its run or much longer than the ones logged. A hold is as long as the longer of
    /// its hold and its wait.
    pub(crate) fn record(&mut self, observation: &Observation, now_ms: u64) -> Option<Line> {
        let purpose = observation.purpose;
        let held_ms = millis(observation.held);
        let waited_ms = millis(observation.waited);
        let length_ms = held_ms.max(waited_ms);
        let Some(run) = self.runs.get_mut(purpose) else {
            self.runs.insert(
                purpose,
                Run {
                    logged_worst_ms: length_ms,
                    counted: 0,
                    counted_worst_ms: 0,
                    counted_worst_wait_ms: 0,
                    window_start_ms: now_ms,
                    last_hold_ms: now_ms,
                },
            );
            return Some(hold_line(observation));
        };
        run.last_hold_ms = now_ms;
        if length_ms >= run.logged_worst_ms.saturating_mul(2) {
            run.logged_worst_ms = length_ms;
            return Some(hold_line(observation));
        }
        run.counted = run.counted.saturating_add(1);
        run.counted_worst_ms = run.counted_worst_ms.max(held_ms);
        run.counted_worst_wait_ms = run.counted_worst_wait_ms.max(waited_ms);
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
                        .field("worstWaitMs", run.counted_worst_wait_ms)
                        .field("windowMs", window_ms),
                );
            }
            run.counted = 0;
            run.counted_worst_ms = 0;
            run.counted_worst_wait_ms = 0;
            run.window_start_ms = now_ms;
            now_ms.saturating_sub(run.last_hold_ms) < SUMMARY_EVERY_MS
        });
        lines
    }
}

fn hold_line(observation: &Observation) -> Line {
    Line::new("engine.core_lock_slow")
        .field("purpose", observation.purpose)
        .field_opt(
            "method",
            (!observation.method.is_empty()).then_some(observation.method),
        )
        .field("heldMs", millis(observation.held))
        .field("waitedMs", millis(observation.waited))
        .field("sqlMs", millis(observation.costs.sql))
        .field("commitMs", millis(observation.costs.commit))
        .field("segmentMs", millis(observation.costs.segment))
        .field("fileMs", millis(observation.costs.file))
}

#[derive(Default)]
struct State {
    slow: SlowHolds,
    usage: Usage,
}

static HOLDS: Mutex<Option<State>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Counts a hold of the engine's lock, and writes the line the rule asks for when it was slow.
pub(crate) fn note(observation: &Observation) {
    let now = now_ms();
    let line = {
        let mut guard = HOLDS.lock().unwrap_or_else(PoisonError::into_inner);
        let state = guard.get_or_insert_with(State::default);
        state.usage.add(observation, now);
        (observation.held >= SLOW || observation.waited >= SLOW)
            .then(|| state.slow.record(observation, now))
            .flatten()
    };
    if let Some(line) = line {
        line.emit();
    }
}

/// What the holds of `purpose` spent so far in the window, for tests.
#[cfg(test)]
pub(crate) fn costs_of(purpose: &str) -> Option<soyeht_platform::hold_cost::Costs> {
    HOLDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|state| state.usage.costs_of(purpose))
}

/// Writes the summaries and the usage of the windows that are over. The tick calls it, outside the
/// lock.
pub(crate) fn emit_due() {
    let now = now_ms();
    let lines = HOLDS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .map(|state| {
            let mut lines = state.slow.due(now);
            lines.extend(state.usage.due(now, SUMMARY_EVERY_MS));
            lines
        })
        .unwrap_or_default();
    for line in lines {
        line.emit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_platform::hold_cost::Costs;
    use std::time::Duration;

    fn json(line: Option<Line>) -> String {
        line.map(|line| line.json()).unwrap_or_default()
    }

    fn held(purpose: &'static str, held_ms: u64) -> Observation {
        waiting(purpose, held_ms, 0)
    }

    fn waiting(purpose: &'static str, held_ms: u64, waited_ms: u64) -> Observation {
        Observation {
            purpose,
            method: "",
            held: Duration::from_millis(held_ms),
            waited: Duration::from_millis(waited_ms),
            costs: Costs::default(),
        }
    }

    #[test]
    fn a_run_logs_its_first_hold_and_counts_the_ones_like_it() {
        let mut holds = SlowHolds::default();
        assert_eq!(
            json(holds.record(&held("tick", 120), 0)),
            r#"{"event":"engine.core_lock_slow","purpose":"tick","heldMs":"120","waitedMs":"0","sqlMs":"0","commitMs":"0","segmentMs":"0","fileMs":"0"}"#
        );
        // Not the first and not twice as long as the 120 ms already logged.
        let logged = (1..=2_000)
            .filter_map(|n| holds.record(&held("tick", 100 + n % 100), n * 20))
            .count();
        assert_eq!(logged, 0);
    }

    #[test]
    fn a_hold_twice_as_long_as_the_longest_logged_is_logged_at_once() {
        let mut holds = SlowHolds::default();
        assert!(holds.record(&held("tick", 120), 0).is_some());
        assert!(holds.record(&held("tick", 239), 10).is_none());
        assert!(holds.record(&held("tick", 240), 20).is_some());
        assert!(holds.record(&held("tick", 400), 30).is_none());
        assert!(holds.record(&held("tick", 480), 40).is_some());
    }

    #[test]
    fn a_long_wait_is_a_slow_hold_and_the_line_says_where_the_time_went() {
        let mut holds = SlowHolds::default();
        let mut observation = waiting("dispatch", 4, 130);
        observation.method = "pane.input";
        observation.costs.commit = Duration::from_millis(3);
        let line = json(holds.record(&observation, 0));
        assert!(line.contains(r#""method":"pane.input""#), "{line}");
        assert!(line.contains(r#""heldMs":"4""#), "{line}");
        assert!(line.contains(r#""waitedMs":"130""#), "{line}");
        assert!(line.contains(r#""commitMs":"3""#), "{line}");
        // The wait counts as the length of the hold for the rule of the run.
        assert!(holds.record(&waiting("dispatch", 1, 259), 1).is_none());
        assert!(holds.record(&waiting("dispatch", 1, 260), 2).is_some());
        assert!(
            holds
                .due(SUMMARY_EVERY_MS)
                .iter()
                .any(|l| l.json().contains(r#""worstWaitMs":"259""#))
        );
    }

    #[test]
    fn each_purpose_has_its_own_run() {
        let mut holds = SlowHolds::default();
        assert!(holds.record(&held("tick", 120), 0).is_some());
        assert!(holds.record(&held("dispatch", 120), 1).is_some());
        assert!(holds.record(&held("tick", 120), 2).is_none());
        assert!(holds.record(&held("dispatch", 120), 3).is_none());
    }

    #[test]
    fn a_window_ends_in_one_summary_with_the_count_and_the_worst() {
        let mut holds = SlowHolds::default();
        assert!(holds.record(&held("tick", 150), 0).is_some());
        for (n, ms) in [110, 190, 130, 105].into_iter().enumerate() {
            assert!(
                holds
                    .record(&held("tick", ms), 1_000 * (n as u64 + 1))
                    .is_none()
            );
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
        assert!(holds.record(&held("tick", 150), 0).is_some());
        // Nothing was counted besides the logged first hold: nothing to sum up.
        assert!(holds.due(SUMMARY_EVERY_MS).is_empty());
        assert!(holds.due(2 * SUMMARY_EVERY_MS).is_empty());
        // The run is over: the next slow hold is a first one again.
        assert!(
            holds
                .record(&held("tick", 110), 2 * SUMMARY_EVERY_MS + 1)
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
            written += usize::from(holds.record(&held("tick", 115 + n % 200), now).is_some());
            written += holds.due(now).len();
        }
        assert!(written <= 2 + 40, "{written} lines for 2000 holds");
        assert!(written >= 38, "a summary per minute: {written}");
    }
}
