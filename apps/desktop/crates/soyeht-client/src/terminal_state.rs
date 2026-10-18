//! What the state of a terminal pane tells the person, the rule of the Mac's
//! `TerminalStatePresentation`: a line above the screen when the process has exited or has gone
//! quiet, and one under it when output was not recorded. Normal activity says nothing.

use std::borrow::Cow;

use soyeht_protocol::PaneState;

/// How loudly a notice speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Information: the process has exited.
    Neutral,
    /// Something is wrong or has stopped.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: Cow<'static, str>,
    pub severity: Severity,
}

impl Notice {
    fn new(text: impl Into<Cow<'static, str>>, severity: Severity) -> Self {
        Self {
            text: text.into(),
            severity,
        }
    }
}

/// The notice a terminal in `state` shows above its screen, if any. A process exit warns only
/// when its status says it failed or was interrupted: a one-shot command that finished cleanly,
/// such as an agent sign-in, only informs, and so does an exit with no recorded status.
pub fn notice(state: &PaneState, exit_code: Option<i64>) -> Option<Notice> {
    match state {
        PaneState::Dead => Some(match exit_code {
            None => Notice::new("Terminal process exited.", Severity::Neutral),
            Some(0) => Notice::new("Terminal process finished.", Severity::Neutral),
            Some(code) if code > 0 => Notice::new(
                format!("Terminal process exited with status {code}."),
                Severity::Warning,
            ),
            Some(_) => Notice::new(
                "Terminal process ended without an exit status.",
                Severity::Warning,
            ),
        }),
        PaneState::NoSignal => Some(Notice::new(
            "The terminal has stopped sending signals. You can keep waiting or press Control-C.",
            Severity::Warning,
        )),
        _ => None,
    }
}

/// The notice under a screen that is missing output the engine no longer retains.
pub const RECORDED_GAP: Notice = Notice {
    text: Cow::Borrowed("Some output was not recorded. The screen contains a gap."),
    severity: Severity::Warning,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::vector;

    #[test]
    fn only_an_exited_or_silent_terminal_has_a_notice() {
        for quiet in [
            PaneState::Idle,
            PaneState::Working,
            PaneState::Starting,
            PaneState::WaitingForYou,
            PaneState::Cancelling,
            PaneState::Recovering,
            PaneState::Unknown,
        ] {
            assert_eq!(notice(&quiet, None), None);
        }
    }

    #[test]
    fn a_clean_exit_never_warns_and_a_failed_one_always_does() {
        let severity = |code| notice(&PaneState::Dead, code).map(|n| n.severity);
        assert_eq!(severity(Some(0)), Some(Severity::Neutral));
        assert_eq!(severity(None), Some(Severity::Neutral));
        for failure in [1, 2, 130, -1] {
            assert_eq!(
                severity(Some(failure)),
                Some(Severity::Warning),
                "{failure}"
            );
        }
    }

    #[test]
    fn the_terminal_notice_is_the_one_in_the_vector() {
        let vector = vector("pane-state-presentation");
        for case in vector["terminalNotices"].as_array().unwrap() {
            let state: PaneState = serde_json::from_value(case["state"].clone()).unwrap();
            let got = notice(&state, case["exitCode"].as_i64());
            assert_eq!(
                got.as_ref().map(|n| n.text.as_ref()),
                case["text"].as_str(),
                "{}",
                case["name"]
            );
            let severity = got.map(|n| match n.severity {
                Severity::Neutral => "neutral",
                Severity::Warning => "warning",
            });
            assert_eq!(severity, case["severity"].as_str(), "{}", case["name"]);
        }
    }
}
