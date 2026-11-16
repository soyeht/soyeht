//! How the avatar agent's persona reaches its harness (ADR 0166, contract 19 section 6).
//!
//! The persona is plain text: who the avatar is, then the person's own instructions. Soyeht adds
//! no behaviour to it. What this module decides is only the channel, and it follows what each
//! harness was measured to honour (`docs/research/avatar-persona-delivery.md`):
//!
//! - **The session's system prompt**, when the adapter advertises the ACP `systemPrompt` capability
//!   (`session/new` and `session/load` carry it, appended to the harness's own prompt). An adapter
//!   that does not advertise it ignores the field without an error, so it is never sent to one.
//! - **A block ahead of the first prompt** otherwise: when a session is created (or the person
//!   changed the persona of an open pane) the state is marked `persona_pending`, and the next
//!   `session/prompt` that is built carries the persona as a text block before the prompt's own.
//!   It is no prompt of the queue: it cannot be removed, paused, or overtaken, it costs no turn, and
//!   it reaches the harness before anything the person wrote.
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
use soyeht_store::avatars::Persona;

/// What the block ahead of a prompt says it is, so the harness does not read it as the person's words.
const BLOCK_LABEL: &str = "Standing instructions for this conversation (they come from the \
                           application, not from the person):";

/// The last line of the block, so a replay of the session can tell where the block ends.
const BLOCK_END: &str = "(End of the standing instructions.)";

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

/// Marks an avatar pane whose adapter has no system prompt as owing its persona to the next prompt.
/// A session that was just created, and a persona that was just changed, are the moments.
pub(crate) fn mark_pending(state: &mut State) {
    state.persona_pending = state.avatar && !accepts_system_prompt(state);
}

/// `prompt`, the `session/prompt` request about to be sent, with the persona as the text block ahead
/// of its own when the pane owes it. The mark is cleared whether or not there was a text to carry.
pub(crate) fn ahead_of(
    store: &impl StorePort,
    state: &mut State,
    workspace_id: &str,
    mut prompt: Value,
) -> Result<Value> {
    if !std::mem::take(&mut state.persona_pending) {
        return Ok(prompt);
    }
    if let (Some(text), Some(blocks)) = (
        text(store, state, workspace_id)?,
        prompt["prompt"].as_array_mut(),
    ) {
        blocks.insert(
            0,
            json!({"type":"text","text":format!("{BLOCK_LABEL}\n\n{text}\n\n{BLOCK_END}")}),
        );
    }
    Ok(prompt)
}

/// `text`, a user message the harness replays from the session, without the persona block the
/// engine put ahead of a prompt: the transcript shows what the person wrote. The block is told by
/// its first and last lines, so it goes whichever persona it carried and whether the harness
/// replays it as a message of its own or joined to the prompt's text.
pub(crate) fn strip_replayed<'a>(state: &State, text: &'a str) -> &'a str {
    if !state.avatar {
        return text;
    }
    text.strip_prefix(BLOCK_LABEL)
        .and_then(|block| block.split_once(BLOCK_END))
        .map_or(text, |(_, after)| after.trim_start_matches('\n'))
}

#[cfg(test)]
mod tests;
