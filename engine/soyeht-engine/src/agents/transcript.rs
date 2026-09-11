use super::{Changes, state::State};
use crate::errors::{self, Result};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, TranscriptKind, TranscriptMessage, TranscriptRole, limits};
use uuid::Uuid;

pub fn message(
    db: &rusqlite::Connection,
    pane: &str,
    seq: u64,
) -> Result<Option<TranscriptMessage>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM (SELECT payload,0 AS priority FROM agent_replay_transcript WHERE pane_id=?1 AND seq=?2 UNION ALL SELECT payload,1 AS priority FROM agent_transcript WHERE pane_id=?1 AND seq=?2) ORDER BY priority LIMIT 1",
            params![pane, seq.to_be_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| errors::storage(e.into()))?;
    raw.map(|raw| {
        serde_json::from_str(&raw)
            .map_err(|_| errors::error(ErrorKind::InternalError, "Invalid transcript record"))
    })
    .transpose()
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
    })
}

pub fn user_prompt(
    state: &mut State,
    generation: u32,
    mut text: &str,
    changes: &mut Changes,
) -> Result<()> {
    // JSON escaping can multiply the UTF-8 body size by six. Keep complete
    // records bounded even when the accepted prompt contains only controls.
    while !text.is_empty() {
        let mut end = text.len().min(32 * 1024);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        changes.messages.push(new_message(
            state,
            generation,
            TranscriptRole::User,
            TranscriptKind::Message,
            text[..end].into(),
            true,
        )?);
        text = &text[end..];
    }
    Ok(())
}
pub fn event(pane: &str, message: &TranscriptMessage, kind: &str) -> Value {
    let mut result = json!({"paneId":pane,"kind":kind,"generation":message.generation,"seq":message.seq,"localId":message.local_id});
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" => {
            result["text"] = json!(message.text);
            result["complete"] = json!(message.complete);
        }
        "tool_call" | "tool_call_update" => result["tool"] = json!(message.tool),
        "plan" => result["entries"] = json!(message.plan),
        "usage" => result["usage"] = json!(message.usage),
        "terminal_output" => {
            if let Some(terminal) = &message.terminal {
                for (key, value) in serde_json::to_value(terminal)
                    .expect("terminal")
                    .as_object()
                    .expect("terminal object")
                {
                    result[key] = value.clone();
                }
            }
        }
        _ => (),
    }
    result
}
pub fn put(
    changes: &mut Changes,
    pane: &str,
    message: TranscriptMessage,
    kind: &str,
) -> Result<()> {
    if serde_json::to_vec(&message)
        .expect("message serialization")
        .len()
        > limits::TRANSCRIPT_MESSAGE_BYTES
    {
        return Err(errors::error(
            ErrorKind::InvalidParams,
            "Normalized message exceeds its 256 KiB limit",
        ));
    }
    if !changes.replay_messages {
        changes
            .events
            .push(("pane.update".into(), event(pane, &message, kind)));
    }
    changes.messages.push(message);
    Ok(())
}

pub fn append_text(
    db: &rusqlite::Connection,
    pane: &str,
    state: &mut State,
    generation: u32,
    thought: bool,
    text: &str,
    changes: &mut Changes,
) -> Result<()> {
    append_text_role(
        db,
        pane,
        state,
        generation,
        if thought { "thought" } else { "message" },
        text,
        changes,
    )
}

pub fn append_text_role(
    db: &rusqlite::Connection,
    pane: &str,
    state: &mut State,
    generation: u32,
    key: &str,
    mut text: &str,
    changes: &mut Changes,
) -> Result<()> {
    let thought = key == "thought";
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
    let mut current = if let Some(seq) = state.open_messages.get(key) {
        message(db, pane, *seq)?
    } else {
        None
    };
    loop {
        let mut item = match current.take() {
            Some(item) => item,
            None => new_message(
                state,
                generation,
                if key == "user" {
                    TranscriptRole::User
                } else {
                    TranscriptRole::Assistant
                },
                kind.clone(),
                String::new(),
                false,
            )?,
        };
        let original = item.text.len();
        item.text.push_str(text);
        if serde_json::to_vec(&item).expect("message").len() <= limits::TRANSCRIPT_MESSAGE_BYTES {
            state
                .open_messages
                .insert(key.into(), item.seq.parse().expect("allocated sequence"));
            put(changes, pane, item, event_kind)?;
            return Ok(());
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
            if serde_json::to_vec(&item).expect("message").len() <= limits::TRANSCRIPT_MESSAGE_BYTES
            {
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
            return Err(errors::error(
                ErrorKind::InvalidParams,
                "Message metadata exceeds its limit",
            ));
        }
        put(changes, pane, item, event_kind)?;
        state.open_messages.remove(key);
        text = &text[count..];
        if text.is_empty() {
            return Ok(());
        }
    }
}

pub fn complete(
    db: &rusqlite::Connection,
    pane: &str,
    state: &mut State,
    changes: &mut Changes,
) -> Result<()> {
    for (key, seq) in std::mem::take(&mut state.open_messages) {
        if let Some(mut item) = message(db, pane, seq)? {
            item.complete = true;
            let kind = if key == "thought" {
                "agent_thought_chunk"
            } else if key == "message" || key == "user" {
                "agent_message_chunk"
            } else if key.starts_with("tool:") {
                "tool_call_update"
            } else {
                continue;
            };
            put(changes, pane, item, kind)?;
        }
    }
    Ok(())
}
