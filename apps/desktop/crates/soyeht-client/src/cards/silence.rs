//! The notice under the transcript while a turn is active and the agent has said nothing.

use crate::timestamp::seconds_since_epoch;
use soyeht_protocol::{PaneState, limits::NO_SIGNAL_SEC};
use std::time::Duration;

pub const CANCEL_TURN: &str = "Cancel turn";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilencePresentation {
    /// Whole seconds since the agent last spoke; none when the engine's time is unreadable or ahead
    /// of this clock.
    pub elapsed_seconds: Option<u64>,
    /// The silence has gone on long enough to warn.
    pub escalated: bool,
}

impl SilencePresentation {
    /// `silent_since` is the engine's timestamp, `now` is seconds since the epoch.
    pub fn new(silent_since: &str, state: Option<&PaneState>, now: f64) -> Self {
        let elapsed = seconds_since_epoch(silent_since)
            .map(|since| now - since)
            .filter(|elapsed| elapsed.is_finite() && *elapsed >= 0.0);
        // The elapsed time is cut to whole seconds, as the Mac does.
        let elapsed_seconds = elapsed.map(|elapsed| elapsed as u64);
        Self {
            elapsed_seconds,
            escalated: state == Some(&PaneState::NoSignal)
                || elapsed_seconds.is_some_and(|seconds| seconds >= NO_SIGNAL_SEC as u64),
        }
    }

    pub fn elapsed(&self) -> Option<String> {
        self.elapsed_seconds
            .map(|seconds| crate::agent::elapsed_text(Duration::from_secs(seconds)))
    }

    pub fn title(&self) -> String {
        match (self.escalated, self.elapsed()) {
            (true, Some(elapsed)) => format!("No output from the agent for {elapsed}"),
            (true, None) => "The agent has produced no output".to_owned(),
            (false, Some(elapsed)) => {
                format!("Waiting for the first agent output \u{b7} {elapsed}")
            }
            (false, None) => "Waiting for the first agent output".to_owned(),
        }
    }

    pub fn detail(&self) -> &'static str {
        match (self.elapsed_seconds, self.escalated) {
            (None, _) => {
                "The turn is active. Elapsed time is unavailable. You can keep waiting or cancel this turn."
            }
            (Some(_), true) => {
                "The turn is still active. You can keep waiting or cancel this turn."
            }
            (Some(_), false) => "The turn is active. Nothing from the agent has arrived yet.",
        }
    }
}
