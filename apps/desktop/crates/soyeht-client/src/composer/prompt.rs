//! Sending what the composer holds to the agent.

use crate::{Command, Connection, State, attachments::Draft};

/// What Return in the composer does with the message it holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Submission {
    /// The engine cannot be reached. The person was told, and the message stays in the composer.
    Refused,
    /// The request that carries the message and its files. Whether the engine's queue takes it is
    /// the caller's to see: the message leaves the composer only once it has.
    Send(Command),
}

impl State {
    /// The request that sends `text` (what the composer holds, ready to send) with `attachments`
    /// to the agent of `pane_id`. A `/skill arguments` line becomes the prompt the harness
    /// understands.
    pub fn submit_prompt(
        &mut self,
        pane_id: &str,
        text: &str,
        attachments: Vec<Draft>,
    ) -> Submission {
        let text = self.slash_catalog(pane_id).prompt(text);
        if self.connection != Connection::Connected {
            self.report_disconnected();
            return Submission::Refused;
        }
        Submission::Send(Command::Prompt {
            pane_id: pane_id.to_owned(),
            text,
            attachments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{attachments::Source, testing::connected};
    use soyeht_protocol::PromptAttachmentKind;

    fn attachment() -> Draft {
        Draft::new(
            PromptAttachmentKind::File,
            "notes.txt".into(),
            "text/plain".into(),
            2,
            Source::Bytes(b"hi".to_vec()),
            None,
        )
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
            })
        );
        assert!(state.problem.is_none());
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
}
