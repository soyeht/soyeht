//! How the avatar agent's persona reaches its harness (ADR 0166, contract 19 section 6).
//!
//! The persona is plain text: who the avatar is, then the person's own instructions. Soyeht adds
//! no behaviour to it. What this module decides is only the channel, and it follows what each
//! harness was measured to honour (`docs/research/avatar-persona-delivery.md`):
//!
//! - **The session's system prompt**, when the adapter advertises the ACP `systemPrompt` capability
//!   (`session/new` and `session/load` carry it, appended to the harness's own prompt). An adapter
//!   that does not advertise it ignores the field without an error, so it is never sent to one.
//! - **A block ahead of a prompt** otherwise: when a session is created the state is marked
//!   `persona_pending`, and so is the state of an open pane whose persona the person changed, on
//!   every adapter (the system prompt of one that has it catches up when its session next opens).
//!   The next `session/prompt` that is built carries the persona as a text block before the prompt's
//!   own, and the pane owes it until the harness has answered that prompt. It is no prompt of the
//!   queue: it cannot be removed, paused, or overtaken, it costs no turn, and it reaches the harness
//!   before anything the person wrote.
//!
//! A file in the project folder (`AGENTS.md`, `CLAUDE.md`) is not used: the harnesses read them,
//! but a workspace can be a folder the person already has with such a file, and Soyeht never writes
//! over the person's files for this. The text is read from the workspace's binding each time it is
//! delivered, so a persona the person changed is the one the next delivery carries. It is never
//! written to a log.

use super::state::State;
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::limits::PROMPT_BYTES;
use soyeht_store::avatars::Persona;

/// What the block ahead of a prompt says it is, so the harness does not read it as the person's words.
const BLOCK_LABEL: &str = "Standing instructions for this conversation (they come from the \
                           application, not from the person):";

/// The last line of the block, so a replay of the session can tell where the block ends.
const BLOCK_END: &str = "(End of the standing instructions.)";
/// The last line of the block as a persona that holds it is delivered: the full stop of that line
/// goes, so the line that ends the block is the engine's alone.
const ESCAPED_END: &str = "(End of the standing instructions)";

fn compose(persona: &Persona) -> String {
    let identity = format!(
        "You are {}, the avatar agent of this project.",
        persona.name
    );
    if persona.text.trim().is_empty() {
        identity
    } else {
        format!("{identity}\n\n{}", persona.text.trim())
    }
}

/// The text the avatar pane's session starts from, or `None` when the pane is not an avatar's or
/// its workspace has no avatar any more.
pub(crate) fn text(
    store: &impl StorePort,
    state: &State,
    workspace_id: &str,
) -> Result<Option<String>> {
    if !state.avatar {
        return Ok(None);
    }
    Ok(store
        .avatar_persona(workspace_id)
        .map_err(errors::storage)?
        .as_ref()
        .map(compose))
}

fn accepts_system_prompt(state: &State) -> bool {
    state
        .initialized
        .as_ref()
        .is_some_and(|initialized| initialized.system_prompt)
}

/// The request of a `session/new` or `session/load` with the persona as the session's system prompt,
/// for an adapter that applies one.
pub(crate) fn system_prompt(state: &State, persona: Option<&str>, mut session: Value) -> Value {
    if let Some(text) = persona.filter(|_| accepts_system_prompt(state)) {
        session["systemPrompt"] = json!({"mode":"append","content":[{"type":"text","text":text}]});
    }
    session
}

/// Marks an avatar pane whose adapter has no system prompt as owing its persona to the next prompt:
/// a session that was just created has not been told, unless its adapter took the system prompt.
pub(crate) fn mark_pending(state: &mut State) {
    state.persona_pending = state.avatar && !accepts_system_prompt(state);
    state.persona_carried_by = None;
}

/// Marks an avatar pane whose persona or avatar was changed as owing the persona to its next prompt,
/// whatever its adapter: the system prompt of one that has it catches up when its session next opens.
pub(crate) fn owe(state: &mut State) {
    state.persona_pending |= state.avatar;
    // A prompt that is streaming carried the persona the pane had: its answer does not settle this one.
    state.persona_carried_by = None;
}

/// `prompt`, the `session/prompt` request about to be sent for the queued prompt `prompt_id`, with
/// the persona as the text block ahead of its own when the pane owes it. The pane keeps owing it:
/// [`answered`] settles it when the harness has the prompt.
pub(crate) fn ahead_of(
    store: &impl StorePort,
    state: &mut State,
    (workspace_id, prompt_id): (&str, &str),
    mut prompt: Value,
) -> Result<Value> {
    if !state.persona_pending {
        return Ok(prompt);
    }
    let Some(text) = text(store, state, workspace_id)? else {
        // The workspace has no avatar any more: nothing is owed.
        state.persona_pending = false;
        return Ok(prompt);
    };
    if let Some(blocks) = prompt["prompt"].as_array_mut() {
        let text = text.replace(BLOCK_END, ESCAPED_END);
        blocks.insert(
            0,
            json!({"type":"text","text":format!("{BLOCK_LABEL}\n\n{text}\n\n{BLOCK_END}")}),
        );
        state.persona_carried_by = Some(prompt_id.to_owned());
    }
    Ok(prompt)
}

/// The harness answered the prompt `prompt_id`. When it carried the persona and the answer is not an
/// error (a turn that was cancelled is an answer), the pane no longer owes it; after an error, or a
/// process that was lost, it still does, and the next prompt carries it.
pub(crate) fn answered(state: &mut State, prompt_id: &str, delivered: bool) {
    if state.persona_carried_by.as_deref() != Some(prompt_id) {
        return;
    }
    state.persona_carried_by = None;
    if delivered {
        state.persona_pending = false;
    }
}

/// How far the replay of a user message is from knowing whether it begins with a persona block.
#[derive(Debug, Clone)]
enum Replay {
    /// What came so far could still be the start of the label.
    Undecided,
    /// The label was seen: everything up to the end line is the block.
    Dropping,
    /// The message is the person's.
    Passing,
}

/// The replay of one user message, chunk by chunk under one message id: an adapter splits a long
/// block, and joins a short one to the text after it.
#[derive(Debug, Clone)]
pub(crate) struct Replayed {
    message_id: Option<String>,
    held: String,
    phase: Replay,
}

impl Replayed {
    /// Whether `message_id` is not the message this is the replay of.
    pub(crate) fn is_another(&self, message_id: Option<&str>) -> bool {
        self.message_id.as_deref() != message_id
    }

    fn new(message_id: Option<&str>) -> Self {
        Self {
            message_id: message_id.map(str::to_owned),
            held: String::new(),
            phase: Replay::Undecided,
        }
    }
}

/// The text a user message that was held back for a message that ended: what could have been the start
/// of a label and never was one, and its message id.
pub(crate) fn finish_replayed(state: &mut State) -> Option<(String, Option<String>)> {
    let replayed = state.persona_replay.take()?;
    let held = matches!(replayed.phase, Replay::Undecided).then_some(replayed.held)?;
    (!held.is_empty()).then_some((held, replayed.message_id))
}

/// `chunk`, a chunk of a replayed user message, without the persona block the engine put ahead of a
/// prompt: the transcript shows what the person wrote. An empty answer says to show nothing yet. A
/// text that begins like the label is held until it is known to be the block, which goes whole, up to
/// its last line, or not, which is shown as it came.
pub(crate) fn filter_replayed(state: &mut State, message_id: Option<&str>, chunk: &str) -> String {
    if !state.avatar {
        return chunk.to_owned();
    }
    let replayed = state
        .persona_replay
        .get_or_insert_with(|| Replayed::new(message_id));
    if replayed.message_id.as_deref() != message_id
        || matches!(replayed.phase, Replay::Passing) && chunk.starts_with(BLOCK_LABEL)
    {
        *replayed = Replayed::new(message_id);
    }
    if matches!(replayed.phase, Replay::Passing) {
        return chunk.to_owned();
    }
    replayed.held.push_str(chunk);
    if replayed.held.len() > BLOCK_LABEL.len() + PROMPT_BYTES + BLOCK_END.len() {
        // An end line that never comes must not swallow what the person wrote after it.
        replayed.phase = Replay::Passing;
        return std::mem::take(&mut replayed.held);
    }
    if matches!(replayed.phase, Replay::Undecided) {
        if replayed.held.starts_with(BLOCK_LABEL) {
            replayed.phase = Replay::Dropping;
        } else if BLOCK_LABEL.starts_with(replayed.held.as_str()) {
            return String::new();
        } else {
            replayed.phase = Replay::Passing;
            return std::mem::take(&mut replayed.held);
        }
    }
    let Some((_, after)) = replayed.held.split_once(BLOCK_END) else {
        return String::new();
    };
    let after = after.trim_start_matches('\n').to_owned();
    replayed.held.clear();
    replayed.phase = Replay::Passing;
    after
}

#[cfg(test)]
mod tests;
