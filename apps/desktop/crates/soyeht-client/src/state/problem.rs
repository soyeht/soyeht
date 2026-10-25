//! The failure line the window shows under its top bar, and its log line.
//!
//! Every failure the person is told about goes through [`State::report`], so each one is also
//! written to the log (`soyeht_platform::log`) with a bounded `kind` and never its message,
//! which can carry a path or an engine's own words.

use super::State;
use soyeht_platform::log;

/// The kinds a reported failure logs as: what could not be done, in one place so that a kind is
/// never typed twice.
pub mod kind {
    pub const AGENT_NOT_STARTED: &str = "agent_not_started";
    pub const ATTACH_PICKER_CLOSED: &str = "attach_picker_closed";
    pub const ATTACH_PICKER_FAILED: &str = "attach_picker_failed";
    pub const ATTENTION_UNREADABLE: &str = "attention_unreadable";
    pub const COMPANION_UNAVAILABLE: &str = "companion_unavailable";
    pub const DISCONNECTED: &str = "disconnected";
    pub const EDITOR_PASTE_REFUSED: &str = "editor_paste_refused";
    pub const EVENT_UNREADABLE: &str = "event_unreadable";
    pub const FOLDER_NAME_NOT_TEXT: &str = "folder_name_not_text";
    pub const FOLDER_PATH_INVALID: &str = "folder_path_invalid";
    pub const FOLDER_PICKER_FAILED: &str = "folder_picker_failed";
    pub const LAYOUT_NOT_EQUALIZED: &str = "layout_not_equalized";
    pub const LAYOUT_NOT_MOVED: &str = "layout_not_moved";
    pub const LAYOUT_NOT_RESIZED: &str = "layout_not_resized";
    pub const LAYOUT_NOT_SAVED: &str = "layout_not_saved";
    pub const LAYOUT_UNREADABLE: &str = "layout_unreadable";
    pub const LOOP_UNREADABLE: &str = "loop_unreadable";
    pub const NEW_AGENT_REFUSED: &str = "new_agent_refused";
    pub const PERMISSION_UNREADABLE: &str = "permission_unreadable";
    pub const QUESTION_UNREADABLE: &str = "question_unreadable";
    pub const REQUEST_FAILED: &str = "request_failed";
    pub const SUPERVISOR_LOST: &str = "supervisor_lost";
    pub const TASKS_UNREADABLE: &str = "tasks_unreadable";
    pub const TRANSCRIPT_UNREADABLE: &str = "transcript_unreadable";
    pub const WRITING_EDIT_REFUSED: &str = "writing_edit_refused";
}

/// The one line the window shows under its top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banner<'a> {
    /// A request that failed.
    Problem(&'a str),
    /// A standing note about this client (a software renderer).
    Notice(&'a str),
}

/// The log line of a failure that was told to the person.
const EVENT: &str = "desktop.problem";

impl State {
    /// Tells the person `text` in the window's failure line, and logs `kind` (one of [`kind`]).
    /// `text` is for the person and stays out of the log.
    pub fn report(&mut self, kind: &str, text: impl Into<String>) {
        log::event(EVENT, &[("kind", kind)]);
        self.problem = Some(text.into());
    }

    /// The engine cannot be reached: said, instead of dropping what the person did.
    pub fn report_disconnected(&mut self) {
        self.report(kind::DISCONNECTED, crate::Error::Disconnected.to_string());
    }

    /// Like [`State::report`], for a failure that carries a cause: `cause` is its
    /// [`log::Failure::failure_kind`], logged after `kind`.
    pub fn report_failure(&mut self, kind: &str, cause: &str, text: impl Into<String>) {
        self.report(&format!("{kind}.{cause}"), text);
    }

    /// A request the engine refused or could not answer: told, and nothing more to do.
    pub(super) fn request_failed(&mut self, cause: &str, message: String) -> Vec<super::Effect> {
        self.report_failure(kind::REQUEST_FAILED, cause, message);
        Vec::new()
    }

    /// The line to show: one at a time, as the Mac shows one failure at a time. A failure comes
    /// before the standing note, which shows again once the failure is dismissed.
    pub fn banner(&self) -> Option<Banner<'_>> {
        self.problem()
            .map(Banner::Problem)
            .or_else(|| self.notice.as_deref().map(Banner::Notice))
    }

    /// The failure the window shows, if any.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// The person dismissed the failure line.
    pub fn dismiss_problem(&mut self) {
        self.problem = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::event::Event;
    use soyeht_platform::log::Failure;
    use std::fs;

    #[test]
    fn a_reported_failure_is_shown_and_logged_by_its_kind_alone() {
        let root = std::env::temp_dir().join(format!("soyeht-problem-log-{}", std::process::id()));
        let path = root.join("logs").join("desktop.log");
        log::to_file(&path).unwrap();

        let mut state = State::default();
        state.report(
            kind::LAYOUT_NOT_SAVED,
            "Couldn't save /home/someone/secret.",
        );
        assert_eq!(state.problem(), Some("Couldn't save /home/someone/secret."));
        state.apply(Event::Problem {
            cause: "not_found".to_owned(),
            message: "No such file /home/someone/other".to_owned(),
        });

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"{"event":"desktop.problem","kind":"layout_not_saved"}"#));
        assert!(text.contains(r#"{"event":"desktop.problem","kind":"request_failed.not_found"}"#));
        assert!(
            !text.contains("someone"),
            "the text of a banner is not logged"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn banners_do_not_stack_a_failure_hides_the_note_until_it_is_dismissed() {
        let mut state = State {
            notice: Some("software".to_owned()),
            ..State::default()
        };
        assert_eq!(state.banner(), Some(Banner::Notice("software")));
        state.report(kind::LAYOUT_NOT_SAVED, "not saved");
        assert_eq!(state.banner(), Some(Banner::Problem("not saved")));
        state.dismiss_problem();
        assert_eq!(state.banner(), Some(Banner::Notice("software")));
        state.notice = None;
        assert_eq!(state.banner(), None);
    }

    #[test]
    fn dismissing_clears_only_the_failure_line() {
        let mut state = State {
            notice: Some("note".to_owned()),
            ..State::default()
        };
        state.report(kind::DISCONNECTED, "Connection interrupted.");
        state.dismiss_problem();
        assert_eq!(state.problem(), None);
        assert_eq!(state.notice.as_deref(), Some("note"));
    }

    #[test]
    fn a_failure_of_the_client_logs_its_class_not_its_message() {
        assert_eq!(Error::Busy.failure_kind(), "busy");
        assert_eq!(Error::Disconnected.failure_kind(), "disconnected");
        assert_eq!(Error::Timeout("pane.list".into()).failure_kind(), "timeout");
        let missing = Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(missing.failure_kind(), "io:NotFound");
    }
}
