//! An agent pane as the view shows it: the messages, and the question the agent is waiting on.

use soyeht_client::State;
use soyeht_protocol::{TranscriptKind, TranscriptMessage, TranscriptRole};

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Person,
    Agent,
    System,
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Message,
    Thought,
    Tool,
    /// Anything the phone does not draw in its own way yet; `text` is what the engine says of it.
    Other,
}

#[derive(uniffi::Record)]
pub struct ConversationItem {
    pub id: String,
    /// Changes whenever the item does, so a list redraws only the rows that changed.
    pub revision: u64,
    pub speaker: Speaker,
    pub kind: ItemKind,
    pub text: String,
    /// The agent is still writing it.
    pub streaming: bool,
}

#[derive(uniffi::Record)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
}

/// What the agent asks the person to allow.
#[derive(uniffi::Record)]
pub struct PermissionQuestion {
    pub request_id: String,
    pub title: String,
    pub detail: String,
    pub options: Vec<PermissionOption>,
}

#[derive(uniffi::Record)]
pub struct Conversation {
    pub items: Vec<ConversationItem>,
    pub question: Option<PermissionQuestion>,
    /// Why the session is not usable, when the engine said so.
    pub notice: Option<String>,
    /// A turn is running.
    pub working: bool,
}

fn item(message: &TranscriptMessage, revision: u64) -> ConversationItem {
    let kind = match message.kind {
        TranscriptKind::Message => ItemKind::Message,
        TranscriptKind::Thought => ItemKind::Thought,
        TranscriptKind::Tool => ItemKind::Tool,
        _ => ItemKind::Other,
    };
    let text = match (&message.tool, message.text.is_empty()) {
        (Some(tool), true) => tool.title.clone(),
        _ => message.text.clone(),
    };
    ConversationItem {
        id: message.local_id.clone(),
        revision,
        speaker: match message.role {
            TranscriptRole::User => Speaker::Person,
            TranscriptRole::Assistant => Speaker::Agent,
            TranscriptRole::System => Speaker::System,
        },
        kind,
        text,
        streaming: !message.complete,
    }
}

pub(crate) fn conversation(state: &State, pane_id: &str) -> Option<Conversation> {
    let view = state.agent(pane_id)?;
    Some(Conversation {
        items: view
            .messages()
            .iter()
            .map(|message| item(message, view.revision_of(&message.local_id)))
            .collect(),
        question: view.permission.as_ref().map(|request| PermissionQuestion {
            request_id: request.request_id.clone(),
            title: request.title.clone(),
            detail: request.detail.clone(),
            options: request
                .options
                .iter()
                .map(|option| PermissionOption {
                    option_id: option.option_id.clone(),
                    name: option.name.clone(),
                })
                .collect(),
        }),
        notice: view.notice.clone(),
        working: view.turn.elapsed(std::time::Instant::now()).is_some(),
    })
}
