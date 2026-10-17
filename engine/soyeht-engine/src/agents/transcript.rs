use super::{Changes, state::State};
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use soyeht_protocol::{
    PromptAttachment, PromptItem, TranscriptKind, TranscriptMessage, TranscriptRole, event, limits,
};
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn message(store: &impl StorePort, pane: &str, seq: u64) -> Result<Option<TranscriptMessage>> {
    store.agent_message(pane, seq).map_err(errors::storage)
}
pub fn new_message(
    state: &mut State,
    generation: u32,
    role: TranscriptRole,
    kind: TranscriptKind,
    text: String,
    complete: bool,
) -> Result<TranscriptMessage> {
    Ok(TranscriptMessage {
        local_id: Uuid::new_v4().to_string(),
        generation,
        seq: state.message_sequence()?.to_string(),
        role,
        kind,
        text,
        complete,
        prompt_id: state.active.as_ref().map(|p| p.prompt_id.clone()),
        tool: None,
        terminal: None,
        plan: None,
        usage: None,
        agent_message: None,
        subagent: None,
        subagent_id: None,
        attachments: None,
    })
}

/// Keys open transcript items: `kind[@subagent][:id]`. A subagent's items
/// never share a key with the parent's, so their text is never merged.
pub fn open_key(kind: &str, subagent: Option<&str>, id: Option<&str>) -> String {
    let mut key = kind.to_owned();
    if let Some(subagent) = subagent {
        key = format!("{key}@{:x}", Sha256::digest(subagent.as_bytes()));
    }
    if let Some(id) = id {
        key = format!("{key}:{id}");
    }
    key
}

/// ACP messageId identifies a message, not a text fragment. Hash the opaque ID
/// to keep the checkpoint key bounded without exposing adapter identifiers.
fn chunk_key(kind: &str, subagent: Option<&str>, message_id: Option<&str>) -> String {
    let message_id = message_id.map(|id| format!("{:x}", Sha256::digest(id.as_bytes())));
    open_key(kind, subagent, message_id.as_deref())
}

/// Adds a file a native replay reported to the user message it belongs to,
/// starting that message when the file comes before any text.
pub fn attach_to_replayed_user(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    generation: u32,
    message_id: Option<&str>,
    attachment: PromptAttachment,
    changes: &mut Changes,
) -> Result<()> {
    let key = chunk_key("user", None, message_id);
    let open = match state.open_messages.get(&key) {
        Some(seq) => message(store, pane, *seq)?,
        None => None,
    };
    let mut item = match open {
        Some(item) => item,
        None => new_message(
            state,
            generation,
            TranscriptRole::User,
            TranscriptKind::Message,
            String::new(),
            false,
        )?,
    };
    let attachments = item.attachments.get_or_insert_with(Vec::new);
    if attachments.len() >= limits::PROMPT_ATTACHMENTS {
        return Ok(());
    }
    attachments.push(attachment);
    state.open_messages.insert(key, sequence(&item.seq)?);
    put(changes, pane, item, "agent_message_chunk")
}

fn open_key_kind(key: &str) -> &str {
    key.split(['@', ':']).next().unwrap_or(key)
}

/// The typed marker for a harness switch; it names the agent that continues.
pub fn handover(state: &mut State, generation: u32) -> Result<TranscriptMessage> {
    let name = state.launch.name.clone();
    new_message(
        state,
        generation,
        TranscriptRole::System,
        TranscriptKind::Handover,
        name,
        true,
    )
}

/// Records an accepted prompt as the pane's user side. A peer agent's message
/// keeps its sender so clients attribute the bubble by name.
pub fn user_prompt(
    state: &mut State,
    generation: u32,
    prompt: &PromptItem,
    changes: &mut Changes,
) -> Result<()> {
    let mut text = prompt.text.as_str();
    let mut attachments = prompt.attachments.clone();
    // JSON escaping can multiply the UTF-8 body size by six. Keep complete
    // records bounded even when the accepted prompt contains only controls.
    while !text.is_empty() {
        // The first part shows what the user attached, so it holds less text.
        let part = if attachments.is_some() { 16 } else { 32 } * 1024;
        let mut end = text.len().min(part);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut message = new_message(
            state,
            generation,
            TranscriptRole::User,
            TranscriptKind::Message,
            text[..end].into(),
            true,
        )?;
        message.agent_message = prompt.agent_message.clone();
        message.attachments = attachments.take();
        fit_attachments(&mut message)?;
        changes.messages.push(message);
        text = &text[end..];
    }
    Ok(())
}

/// Keeps a message with attachments within its stored bound. Thumbnails are
/// dropped from the last attachment back; names, sizes and paths stay.
fn fit_attachments(message: &mut TranscriptMessage) -> Result<()> {
    let mut thumbnails = message.attachments.as_ref().map_or(0, Vec::len);
    while errors::json_bytes(message)?.len() > limits::TRANSCRIPT_MESSAGE_BYTES {
        match message.attachments.as_mut() {
            Some(attachments) if thumbnails > 0 => {
                thumbnails -= 1;
                attachments[thumbnails].thumbnail = None;
            }
            _ => {
                message.attachments = None;
                break;
            }
        }
    }
    Ok(())
}
pub fn event(pane: &str, message: &TranscriptMessage, kind: &str) -> Value {
    let mut result = json!({"paneId":pane,"kind":kind,"generation":message.generation,"seq":message.seq,"localId":message.local_id});
    if let Some(subagent) = &message.subagent_id {
        result["subagentId"] = json!(subagent);
    }
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" | "system_message" => {
            result["text"] = json!(message.text);
            result["complete"] = json!(message.complete);
        }
        "tool_call" | "tool_call_update" => result["tool"] = json!(message.tool),
        "plan" => result["entries"] = json!(message.plan),
        "subagent" => result["subagent"] = json!(message.subagent),
        "usage" => result["usage"] = json!(message.usage),
        "terminal_output" => {
            if let Some(terminal) = &message.terminal {
                result["terminalId"] = json!(terminal.terminal_id);
                result["bytes"] = json!(terminal.bytes);
                result["offset"] = json!(terminal.offset);
                result["truncated"] = json!(terminal.truncated);
                if let Some(exit_code) = terminal.exit_code {
                    result["exitCode"] = json!(exit_code);
                }
            }
        }
        _ => (),
    }
    result
}

pub fn sequence(value: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| errors::internal("Allocated transcript sequence is invalid"))
}
pub fn put(
    changes: &mut Changes,
    pane: &str,
    message: TranscriptMessage,
    kind: &str,
) -> Result<()> {
    if errors::json_bytes(&message)?.len() > limits::TRANSCRIPT_MESSAGE_BYTES {
        return Err(errors::invalid(
            "Normalized message exceeds its 256 KiB limit",
        ));
    }
    if !changes.replay_messages {
        changes
            .events
            .push((event::PANE_UPDATE.into(), event(pane, &message, kind)));
    }
    changes.messages.push(message);
    Ok(())
}

pub fn append_text(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    generation: u32,
    thought: bool,
    chunk: TextChunk<'_>,
    changes: &mut Changes,
) -> Result<Option<u64>> {
    append_text_role(
        store,
        pane,
        state,
        generation,
        if thought { "thought" } else { "message" },
        chunk,
        changes,
    )
}

pub struct TextChunk<'a> {
    pub text: &'a str,
    pub message_id: Option<&'a str>,
    /// The subagent session that wrote the text, if not the parent agent.
    pub subagent: Option<&'a str>,
}

/// Appends streamed text and returns the sequence of the message it ends in.
pub fn append_text_role(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    generation: u32,
    key: &str,
    chunk: TextChunk<'_>,
    changes: &mut Changes,
) -> Result<Option<u64>> {
    let mut text = chunk.text;
    // Native replay may include an empty response for a cancelled turn.
    // It is a role boundary, not an additional visible conversation message.
    if text.is_empty() {
        return Ok(None);
    }
    let thought = key == "thought";
    let key = chunk_key(key, chunk.subagent, chunk.message_id);
    let event_kind = if thought {
        "agent_thought_chunk"
    } else {
        "agent_message_chunk"
    };
    let kind = if thought {
        TranscriptKind::Thought
    } else {
        TranscriptKind::Message
    };
    let mut current = if let Some(seq) = state.open_messages.get(&key) {
        message(store, pane, *seq)?
    } else {
        None
    };
    loop {
        let mut item = match current.take() {
            Some(item) => item,
            None => new_message(
                state,
                generation,
                if open_key_kind(&key) == "user" {
                    TranscriptRole::User
                } else {
                    TranscriptRole::Assistant
                },
                kind.clone(),
                String::new(),
                false,
            )?,
        };
        item.subagent_id = chunk.subagent.map(Into::into);
        let original = item.text.len();
        item.text.push_str(text);
        if errors::json_bytes(&item)?.len() <= limits::TRANSCRIPT_MESSAGE_BYTES {
            let seq = sequence(&item.seq)?;
            state.open_messages.insert(key.clone(), seq);
            put(changes, pane, item, event_kind)?;
            return Ok(Some(seq));
        }
        item.text.truncate(original);
        let boundaries = text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        let mut low = 0;
        let mut high = boundaries.len() - 1;
        while low < high {
            let middle = (low + high).div_ceil(2);
            item.text.truncate(original);
            item.text.push_str(&text[..boundaries[middle]]);
            if errors::json_bytes(&item)?.len() <= limits::TRANSCRIPT_MESSAGE_BYTES {
                low = middle
            } else {
                high = middle - 1
            }
        }
        let count = boundaries[low];
        item.text.truncate(original);
        item.text.push_str(&text[..count]);
        item.complete = true;
        if item.text.is_empty() {
            return Err(errors::invalid("Message metadata exceeds its limit"));
        }
        let seq = sequence(&item.seq)?;
        put(changes, pane, item, event_kind)?;
        state.open_messages.remove(&key);
        text = &text[count..];
        if text.is_empty() {
            return Ok(Some(seq));
        }
    }
}

pub fn complete(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    changes: &mut Changes,
) -> Result<()> {
    complete_where(store, pane, state, changes, |_| true)
}

/// Ends the streamed text of one speaker, the parent or a subagent, so text it
/// writes after a subagent row starts below that row.
pub fn end_text(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    subagent: Option<&str>,
    changes: &mut Changes,
) -> Result<()> {
    let bases = ["message", "thought"].map(|kind| open_key(kind, subagent, None));
    complete_where(store, pane, state, changes, |key| {
        bases.iter().any(|base| {
            key.strip_prefix(base.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
        })
    })
}

fn complete_where(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    changes: &mut Changes,
    selected: impl Fn(&str) -> bool,
) -> Result<()> {
    let (done, open): (BTreeMap<_, _>, _) = std::mem::take(&mut state.open_messages)
        .into_iter()
        .partition(|(key, _)| selected(key));
    state.open_messages = open;
    for (key, seq) in done {
        if let Some(mut item) = message(store, pane, seq)? {
            item.complete = true;
            let kind = match open_key_kind(&key) {
                "thought" => "agent_thought_chunk",
                "message" => "agent_message_chunk",
                "user" => {
                    // Only native replay assembles user text from chunks.
                    super::peer::restore_replayed(&mut item);
                    "agent_message_chunk"
                }
                "tool" => "tool_call_update",
                _ => continue,
            };
            put(changes, pane, item, kind)?;
        }
    }
    Ok(())
}

/// The harness process ended: finish what it left open. A replay in progress
/// is abandoned instead, so nothing of it is published.
pub fn close(
    store: &impl StorePort,
    pane: &str,
    state: &mut State,
    changes: &mut Changes,
) -> Result<()> {
    if state.phase == super::state::Phase::Loading {
        state.open_messages.clear();
        state.subagents.clear();
        return Ok(());
    }
    complete(store, pane, state, changes)?;
    super::subagents::disconnect_all(store, pane, state, changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::{SubagentRun, SubagentStatus};

    #[test]
    fn a_prompt_with_the_largest_attachments_keeps_its_first_part_within_bounds() {
        let attachment = soyeht_protocol::PromptAttachment {
            kind: soyeht_protocol::PromptAttachmentKind::Image,
            name: "shot.png".into(),
            mime_type: "image/png".into(),
            bytes: "1".into(),
            attachment_id: None,
            data: None,
            path: Some(format!("/{}", "p".repeat(4000))),
            thumbnail: Some("A".repeat(8192)),
        };
        let mut message = item(TranscriptKind::Message);
        message.text = "\u{1}".repeat(16 * 1024);
        message.attachments = Some(vec![attachment; limits::PROMPT_ATTACHMENTS]);
        fit_attachments(&mut message).unwrap();
        assert!(errors::json_bytes(&message).unwrap().len() <= limits::TRANSCRIPT_MESSAGE_BYTES);
        let kept = message.attachments.unwrap();
        assert_eq!(kept.len(), limits::PROMPT_ATTACHMENTS);
        assert!(kept[0].thumbnail.is_some() || kept.iter().all(|a| a.thumbnail.is_none()));
        assert!(kept.last().unwrap().thumbnail.is_none());
    }

    fn item(kind: TranscriptKind) -> TranscriptMessage {
        TranscriptMessage {
            local_id: "local".into(),
            generation: 1,
            seq: "4".into(),
            role: TranscriptRole::Assistant,
            kind,
            text: String::new(),
            complete: false,
            prompt_id: None,
            tool: None,
            terminal: None,
            plan: None,
            usage: None,
            agent_message: None,
            subagent: None,
            subagent_id: None,
            attachments: None,
        }
    }

    #[test]
    fn live_updates_name_the_subagent_that_owns_them() {
        let mut row = item(TranscriptKind::Subagent);
        row.subagent = Some(SubagentRun {
            subagent_id: "child".into(),
            name: "Explore".into(),
            task: "Look around".into(),
            status: SubagentStatus::Running,
            model: None,
            started_at: None,
            finished_at: None,
            summary: None,
        });
        let update = event("pane", &row, "subagent");
        assert_eq!(update["subagent"]["subagentId"], "child");
        assert!(update.get("subagentId").is_none());
        let mut child = item(TranscriptKind::Message);
        child.subagent_id = Some("child".into());
        child.text = "Found it".into();
        let update = event("pane", &child, "agent_message_chunk");
        assert_eq!(update["subagentId"], "child");
        assert_eq!(update["text"], "Found it");
    }

    #[test]
    fn subagent_text_keys_never_collide_with_the_parent() {
        let parent = open_key("message", None, Some("id"));
        let child = open_key("message", Some("child"), Some("id"));
        assert_ne!(parent, child);
        assert_eq!(open_key_kind(&parent), "message");
        assert_eq!(open_key_kind(&child), "message");
        assert_eq!(open_key("tool", None, Some("call")), "tool:call");
    }
}
