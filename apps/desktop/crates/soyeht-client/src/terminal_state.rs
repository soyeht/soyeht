//! What the state of a terminal pane tells the person, the rule of the Mac's
//! `TerminalStatePresentation`: a line above the screen when the process has exited or has gone
//! quiet, and one under it when output was not recorded. Normal activity says nothing.

use soyeht_protocol::PaneState;

/// How loudly a notice speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Information: the process has exited.
    Neutral,
    /// Something is wrong or has stopped.
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub text: &'static str,
    pub severity: Severity,
}

/// The notice a terminal in `state` shows above its screen, if any.
pub fn notice(state: &PaneState) -> Option<Notice> {
    match state {
        PaneState::Dead => Some(Notice {
            text: "Terminal process exited.",
            severity: Severity::Neutral,
        }),
        PaneState::NoSignal => Some(Notice {
            text: "The terminal has stopped sending signals. You can keep waiting or press Control-C.",
            severity: Severity::Warning,
        }),
        _ => None,
    }
}

/// The notice under a screen that is missing output the engine no longer retains.
pub const RECORDED_GAP: Notice = Notice {
    text: "Some output was not recorded. The screen contains a gap.",
    severity: Severity::Warning,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_exited_or_silent_terminal_has_a_notice() {
        assert_eq!(
            notice(&PaneState::Dead).map(|n| n.severity),
            Some(Severity::Neutral)
        );
        assert_eq!(
            notice(&PaneState::NoSignal).map(|n| n.severity),
            Some(Severity::Warning)
        );
        for quiet in [
            PaneState::Idle,
            PaneState::Working,
            PaneState::Starting,
            PaneState::WaitingForYou,
            PaneState::Cancelling,
            PaneState::Recovering,
            PaneState::Unknown,
        ] {
            assert_eq!(notice(&quiet), None);
        }
    }
}
