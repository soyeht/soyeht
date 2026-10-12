//! The messages waiting for the agent while it works.

use soyeht_protocol::{AgentMessageSource, PromptItem};

pub const RESUME: &str = "Resume";
pub const REMOVE: &str = "Remove queued message";
pub const SEND_NOW: &str = "Send now";
pub const SEND_NOW_HINT: &str = "This agent only reads new messages between turns";

/// Who sent a peer message, by name; the pane identifier only routes replies.
pub fn agent_message_name(source: &AgentMessageSource) -> &str {
    if source.name.trim().is_empty() {
        &source.harness
    } else {
        &source.name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub prompt_id: String,
    /// "From aria" for a message another agent sent.
    pub sender: Option<String>,
    pub text: String,
    pub attachment_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePresentation {
    pub summary: String,
    pub paused: bool,
    pub rows: Vec<QueueRow>,
}

impl QueuePresentation {
    /// The card is drawn while something waits and while the queue is paused.
    pub fn shown(items: &[PromptItem], paused: bool) -> bool {
        !items.is_empty() || paused
    }

    pub fn new(items: &[PromptItem], paused: bool) -> Self {
        let from_agents = items
            .iter()
            .filter(|item| item.agent_message.is_some())
            .count();
        let count = if from_agents > 0 {
            format!(
                "{from_agents} {} waiting",
                if from_agents == 1 {
                    "message"
                } else {
                    "messages"
                }
            )
        } else {
            format!("{} queued", items.len())
        };
        Self {
            summary: if paused {
                format!("{count} \u{b7} Queue paused after interruption")
            } else {
                count
            },
            paused,
            rows: items
                .iter()
                .map(|item| QueueRow {
                    prompt_id: item.prompt_id.clone(),
                    sender: item
                        .agent_message
                        .as_ref()
                        .map(|source| format!("From {}", agent_message_name(source))),
                    text: item.text.clone(),
                    attachment_count: item
                        .attachments
                        .as_ref()
                        .map(Vec::len)
                        .filter(|count| *count > 0),
                })
                .collect(),
        }
    }
}
