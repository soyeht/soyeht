//! An agent pane as the view shows it: the messages, and the question the agent is waiting on.

use soyeht_client::State;
use soyeht_protocol::{
    PermissionOptionKind, ToolStatus, TranscriptKind, TranscriptMessage, TranscriptRole,
};

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

/// How far a tool call is, for the word and mark at the end of its row.
#[derive(Debug, uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Pending,
    Running,
    Done,
    Failed,
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
    /// How far the call is, when the item is a tool call.
    pub tool_state: Option<ToolState>,
    /// The agent that sent the message, when it came from another agent pane and not from the person.
    pub from_agent: Option<String>,
}

/// What an option does, so a view can tell the one that allows from the one that refuses.
#[derive(Debug, uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    Allow,
    AllowAlways,
    Reject,
    /// A kind this build does not know: drawn like a neutral option, never as a grant.
    Other,
}

#[derive(uniffi::Record)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: OptionKind,
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

/// How far a tool call is: a status this build does not know is under way until the message is
/// complete, then done.
fn tool_state(status: &ToolStatus, complete: bool) -> ToolState {
    match status {
        ToolStatus::Pending => ToolState::Pending,
        ToolStatus::InProgress => ToolState::Running,
        ToolStatus::Completed => ToolState::Done,
        ToolStatus::Failed => ToolState::Failed,
        ToolStatus::Unknown if complete => ToolState::Done,
        ToolStatus::Unknown => ToolState::Running,
    }
}

/// What an option of the wire does here: a kind this build does not know is `Other`, which a view
/// draws like a neutral option, so no reader takes it for a grant.
fn option_kind(kind: &PermissionOptionKind) -> OptionKind {
    match kind {
        PermissionOptionKind::AllowOnce => OptionKind::Allow,
        PermissionOptionKind::AllowAlways => OptionKind::AllowAlways,
        PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways => OptionKind::Reject,
        PermissionOptionKind::Unknown => OptionKind::Other,
    }
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
        tool_state: message
            .tool
            .as_ref()
            .map(|tool| tool_state(&tool.status, message.complete)),
        from_agent: message
            .agent_message
            .as_ref()
            .map(|source| source.name.clone()),
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
                    kind: option_kind(&option.kind),
                })
                .collect(),
        }),
        notice: view.notice.clone(),
        working: view.turn.elapsed(std::time::Instant::now()).is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_tool_statuses_keep_their_state() {
        for (status, state) in [
            (ToolStatus::Pending, ToolState::Pending),
            (ToolStatus::InProgress, ToolState::Running),
            (ToolStatus::Completed, ToolState::Done),
            (ToolStatus::Failed, ToolState::Failed),
        ] {
            assert_eq!(tool_state(&status, false), state);
            assert_eq!(tool_state(&status, true), state);
        }
    }

    #[test]
    fn a_tool_status_this_build_does_not_know_runs_then_is_done() {
        // This crate is built with `open-enums`, so a value of a later engine is kept.
        let paused: ToolStatus = serde_json::from_value(serde_json::json!("paused")).unwrap();
        assert_eq!(paused, ToolStatus::Unknown);
        assert_eq!(tool_state(&paused, false), ToolState::Running);
        assert_eq!(tool_state(&paused, true), ToolState::Done);
    }

    #[test]
    fn a_permission_option_this_build_does_not_know_is_no_grant() {
        let future: PermissionOptionKind =
            serde_json::from_value(serde_json::json!("allow_everything")).unwrap();
        assert_eq!(future, PermissionOptionKind::Unknown);
        assert_eq!(option_kind(&future), OptionKind::Other);
        assert_ne!(option_kind(&future), OptionKind::Allow);
        assert_ne!(option_kind(&future), OptionKind::AllowAlways);
        assert_ne!(option_kind(&future), OptionKind::Reject);
    }

    #[test]
    fn the_known_permission_options_keep_their_kind() {
        for (wire, kind) in [
            (PermissionOptionKind::AllowOnce, OptionKind::Allow),
            (PermissionOptionKind::AllowAlways, OptionKind::AllowAlways),
            (PermissionOptionKind::RejectOnce, OptionKind::Reject),
            (PermissionOptionKind::RejectAlways, OptionKind::Reject),
        ] {
            assert_eq!(option_kind(&wire), kind);
        }
    }
}
