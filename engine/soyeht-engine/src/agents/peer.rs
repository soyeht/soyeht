//! The native form of a message queued by another agent pane.
//!
//! The queue and the transcript keep the sender's text and its
//! `AgentMessageSource`; clients label the bubble with the sender's name.
//! Only the prompt sent to the harness carries the reply route, and a native
//! replay of that prompt is folded back into the same attributed form.

use soyeht_protocol::{AgentMessageSource, PromptItem, TranscriptMessage};
use std::borrow::Cow;

const PREFIX: &str = "Message from the Soyeht agent ";
const BODY: &str = ".\n\n";

/// Names the exact server so a harness that also loads other Soyeht servers
/// replies through this pane's own binding.
fn route() -> String {
    format!(
        "). Reply with the message_agent tool of the {} MCP server using targetPaneId=",
        super::mcp::SERVER_NAME
    )
}

/// The text a harness receives for a queued prompt.
pub(crate) fn native_text(prompt: &PromptItem) -> Cow<'_, str> {
    match &prompt.agent_message {
        None => Cow::Borrowed(&prompt.text),
        Some(source) => Cow::Owned(format!(
            "{PREFIX}{} ({}{}{}{BODY}{}",
            source.name,
            source.harness,
            route(),
            source.pane_id,
            prompt.text,
        )),
    }
}

/// Restores the sender and the sender's text of a replayed peer prompt.
pub(crate) fn restore_replayed(message: &mut TranscriptMessage) {
    let Some((source, text)) = parse(&message.text) else {
        return;
    };
    message.text = text.into();
    message.agent_message = Some(source);
}

fn parse(text: &str) -> Option<(AgentMessageSource, &str)> {
    let rest = text.strip_prefix(PREFIX)?;
    let (header, body) = rest.split_once(BODY)?;
    let (sender, pane_id) = header.rsplit_once(route().as_str())?;
    let (name, harness) = sender.rsplit_once(" (")?;
    Some((
        AgentMessageSource {
            pane_id: pane_id.into(),
            name: name.into(),
            harness: harness.into(),
        },
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::{PromptOrigin, PromptStatus, TranscriptKind, TranscriptRole};

    fn prompt(text: &str, agent_message: Option<AgentMessageSource>) -> PromptItem {
        PromptItem {
            prompt_id: "prompt".into(),
            text: text.into(),
            origin: PromptOrigin::Automatic,
            status: PromptStatus::Queued,
            queued_at: "now".into(),
            generation: None,
            attachments: None,
            agent_message,
        }
    }

    #[test]
    fn only_the_native_prompt_carries_the_reply_route_and_replay_restores_the_sender() {
        let source = AgentMessageSource {
            pane_id: "pane-7".into(),
            name: "Claude (review)".into(),
            harness: "Claude Agent".into(),
        };
        let queued = prompt("ping.\n\nsecond line", Some(source.clone()));
        let native = native_text(&queued);
        assert!(native.contains("targetPaneId=pane-7"));
        assert!(native.ends_with("\n\nping.\n\nsecond line"));
        assert_eq!(native_text(&prompt("typed", None)), "typed");

        let mut replayed = TranscriptMessage {
            local_id: "local".into(),
            generation: 1,
            seq: "1".into(),
            role: TranscriptRole::User,
            kind: TranscriptKind::Message,
            text: native.into_owned(),
            complete: true,
            prompt_id: None,
            tool: None,
            terminal: None,
            plan: None,
            usage: None,
            agent_message: None,
            subagent: None,
            subagent_id: None,
        };
        restore_replayed(&mut replayed);
        assert_eq!(replayed.text, "ping.\n\nsecond line");
        assert_eq!(replayed.agent_message, Some(source));

        let mut typed = replayed.clone();
        typed.text = "Message from a colleague".into();
        typed.agent_message = None;
        restore_replayed(&mut typed);
        assert_eq!(typed.text, "Message from a colleague");
        assert!(typed.agent_message.is_none());
    }
}
