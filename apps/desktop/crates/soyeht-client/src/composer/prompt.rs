//! Sending what the composer holds to the agent, and taking it back when the engine does not
//! accept it.
//!
//! The composer empties as soon as the message is queued, so that the person can go on typing. The
//! command keeps what was typed and the files (`Command::Prompt::typed`): when the engine refuses
//! the message, does not answer in time, or the connection goes, they come back
//! (`Event::PromptReturned`, then [`State::take_returned_prompts`]) and the window puts them in
//! the composer of the pane. What the person typed meanwhile is never overwritten: the returned
//! text goes first, a blank line follows, then the newer text ([`returned_text`]); the returned
//! files go before the newer ones, within the limits of a message ([`Staged::restore`]). A pane
//! that was closed meanwhile does not get its message back.
//!
//! [`Staged::restore`]: crate::attachments::Staged::restore

use crate::{Command, Connection, Effect, State, attachments::Draft};

/// What Return in the composer does with the message it holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Submission {
    /// There is nothing but space in the composer: nothing is sent.
    Blank,
    /// The engine cannot be reached. The person was told, and the message stays in the composer.
    Refused,
    /// The request that carries the message and its files. Whether the engine's queue takes it is
    /// the caller's to see: the message leaves the composer only once it has, and comes back if
    /// the engine then does not take it.
    Send(Command),
}

/// A message the engine did not take, to put back in the composer of its pane.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnedPrompt {
    pub pane_id: String,
    /// What the composer held, exactly as it was typed.
    pub typed: String,
    pub attachments: Vec<Draft>,
}

/// What the composer holds once `returned`, a message the engine did not take, is back and
/// `newer` is what was typed since: the returned text, a blank line, then the newer text, which is
/// never overwritten.
pub fn returned_text(returned: &str, newer: &str) -> String {
    if newer.trim().is_empty() {
        returned.to_owned()
    } else {
        format!("{returned}\n\n{newer}")
    }
}

impl State {
    /// The request that sends `typed` (what the composer holds) with `attachments` to the agent of
    /// `pane_id`. A `/skill arguments` line becomes the prompt the harness understands.
    pub fn submit_prompt(
        &mut self,
        pane_id: &str,
        typed: &str,
        attachments: Vec<Draft>,
    ) -> Submission {
        let text = typed.trim();
        if text.is_empty() {
            return Submission::Blank;
        }
        let text = self.slash_catalog(pane_id).prompt(text);
        if self.connection != Connection::Connected {
            self.report_disconnected();
            return Submission::Refused;
        }
        Submission::Send(Command::Prompt {
            pane_id: pane_id.to_owned(),
            text,
            attachments,
            typed: Some(typed.to_owned()),
        })
    }

    /// The engine did not take a message sent from the composer of `pane_id`. Messages that
    /// failed one after the other come back in the order they were sent.
    pub(crate) fn return_prompt(&mut self, returned: ReturnedPrompt) -> Vec<Effect> {
        if self.pane(&returned.pane_id).is_some() {
            match self
                .returned_prompts
                .iter_mut()
                .find(|held| held.pane_id == returned.pane_id)
            {
                Some(held) => {
                    held.typed = returned_text(&held.typed, &returned.typed);
                    held.attachments.extend(returned.attachments);
                }
                None => self.returned_prompts.push(returned),
            }
        }
        Vec::new()
    }

    /// The messages the engine did not take, once, for the window to put back. A pane that is gone
    /// by now gets nothing.
    pub fn take_returned_prompts(&mut self) -> Vec<ReturnedPrompt> {
        let mut returned = std::mem::take(&mut self.returned_prompts);
        returned.retain(|held| self.pane(&held.pane_id).is_some());
        returned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Error,
        attachments::{Source, Staged},
        testing::connected,
    };
    use soyeht_protocol::PromptAttachmentKind;

    fn file(name: &str) -> Draft {
        Draft::new(
            PromptAttachmentKind::File,
            name.into(),
            "text/plain".into(),
            2,
            Source::Bytes(b"hi".to_vec()),
            None,
        )
    }

    fn attachment() -> Draft {
        file("notes.txt")
    }

    /// The failures that end a send without the engine having taken the message: it did not
    /// answer in time, the connection went, or the queue was full. A refusal is the same path.
    fn failures() -> Vec<Error> {
        vec![
            Error::Timeout("pane.prompt".into()),
            Error::Disconnected,
            Error::Busy,
            Error::Invalid("a refusal"),
        ]
    }

    /// Sends `typed` with `files` from pane `a` and has it fail for `error`.
    fn sent_and_failed(state: &mut State, typed: &str, files: Vec<Draft>, error: &Error) {
        let Submission::Send(command) = state.submit_prompt("a", typed, files) else {
            panic!("a connected engine is sent the message");
        };
        let event = command.failure(error).expect("a failed prompt is reported");
        assert!(state.apply(event).is_empty());
    }

    #[test]
    fn a_message_is_sent_with_its_files_to_the_pane_it_was_written_in() {
        let mut state = connected(&[("a", "agent")]);
        let files = vec![attachment()];
        assert_eq!(
            state.submit_prompt("a", "hello", files.clone()),
            Submission::Send(Command::Prompt {
                pane_id: "a".into(),
                text: "hello".into(),
                attachments: files,
                typed: Some("hello".into()),
            })
        );
        assert!(state.problem.is_none());
    }

    #[test]
    fn the_engine_is_sent_the_trimmed_line_and_the_composer_keeps_what_was_typed() {
        let mut state = connected(&[("a", "agent")]);
        let Submission::Send(Command::Prompt { text, typed, .. }) =
            state.submit_prompt("a", "  hello\n", Vec::new())
        else {
            panic!("sent");
        };
        assert_eq!(
            (text.as_str(), typed.as_deref()),
            ("hello", Some("  hello\n"))
        );
    }

    #[test]
    fn a_composer_with_nothing_but_space_sends_nothing() {
        let mut state = connected(&[("a", "agent")]);
        assert_eq!(
            state.submit_prompt("a", " \n", Vec::new()),
            Submission::Blank
        );
    }

    #[test]
    fn a_dead_connection_refuses_the_message_and_says_so() {
        let mut state = State::default();
        assert_eq!(
            state.submit_prompt("a", "hello", Vec::new()),
            Submission::Refused
        );
        assert!(state.problem.is_some());
    }

    #[test]
    fn a_message_the_engine_did_not_take_is_not_lost() {
        for error in failures() {
            let mut state = connected(&[("a", "agent")]);
            let files = vec![attachment()];
            sent_and_failed(&mut state, "  hello\n", files.clone(), &error);
            assert_eq!(
                state.take_returned_prompts(),
                vec![ReturnedPrompt {
                    pane_id: "a".into(),
                    typed: "  hello\n".into(),
                    attachments: files,
                }],
                "{error:?}: exactly what was sent, text and files"
            );
            assert!(state.take_returned_prompts().is_empty(), "once");
        }
    }

    #[test]
    fn a_message_a_composer_did_not_send_has_nothing_to_go_back_to() {
        let command = Command::Prompt {
            pane_id: "a".into(),
            text: "hello".into(),
            attachments: Vec::new(),
            typed: None,
        };
        assert!(command.failure(&Error::Disconnected).is_none());
    }

    #[test]
    fn messages_that_fail_one_after_the_other_come_back_in_the_order_they_were_sent() {
        let mut state = connected(&[("a", "agent")]);
        sent_and_failed(&mut state, "first", vec![file("one")], &Error::Disconnected);
        sent_and_failed(
            &mut state,
            "second",
            vec![file("two")],
            &Error::Disconnected,
        );
        let returned = state.take_returned_prompts();
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].typed, "first\n\nsecond");
        let names: Vec<_> = returned[0]
            .attachments
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(names, ["one", "two"]);
    }

    #[test]
    fn a_pane_closed_meanwhile_does_not_get_its_message_back() {
        let mut state = connected(&[("a", "agent"), ("b", "agent")]);
        sent_and_failed(&mut state, "hello", Vec::new(), &Error::Disconnected);
        state.apply(crate::testing::event(
            soyeht_protocol::EventName::PaneClosed,
            serde_json::json!({"paneId": "a", "workspaceId": "w"}),
        ));
        assert!(state.pane("a").is_none());
        assert!(state.take_returned_prompts().is_empty());
        // And one that fails after the pane is gone is not kept.
        let mut state = connected(&[("b", "agent")]);
        sent_and_failed(&mut state, "hello", Vec::new(), &Error::Disconnected);
        assert!(
            state
                .take_returned_prompts()
                .iter()
                .all(|p| p.pane_id != "a")
        );
    }

    #[test]
    fn what_was_typed_since_is_never_overwritten() {
        assert_eq!(returned_text("sent", ""), "sent");
        assert_eq!(returned_text("sent", "  \n"), "sent", "space is not text");
        assert_eq!(returned_text("sent", "more"), "sent\n\nmore");
        assert_eq!(returned_text("  sent\n", "more"), "  sent\n\n\nmore");
    }

    #[test]
    fn returned_files_go_before_the_ones_attached_since() {
        let mut staged = Staged::default();
        staged.add(vec![Ok(file("newer"))]);
        staged.restore(vec![file("sent")]);
        let names: Vec<_> = staged.items().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["sent", "newer"]);
        assert!(staged.notice().is_none());
    }

    #[test]
    fn returned_files_win_when_they_and_the_newer_ones_do_not_all_fit() {
        let limit = soyeht_protocol::limits::PROMPT_ATTACHMENTS;
        let mut staged = Staged::default();
        staged.add((0..limit).map(|n| Ok(file(&format!("new{n}")))).collect());
        staged.restore(vec![file("sent")]);
        let names: Vec<_> = staged.items().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names.len(), limit);
        assert_eq!(names[0], "sent");
        assert!(
            staged.notice().is_some(),
            "the person is told what did not fit"
        );
    }
}
