//! How the avatar agent's persona reaches its harness (ADR 0166, contract 19 section 6).
//!
//! The persona is plain text: who the avatar is, then the person's own instructions. Soyeht adds
//! no behaviour to it. What this module decides is only the channel, and it follows what each
//! harness was measured to honour (`docs/research/avatar-persona-delivery.md`):
//!
//! - **The session's system prompt**, when the adapter advertises the ACP `systemPrompt`
//!   capability (`session/new` and `session/load` carry it, appended to the harness's own prompt).
//!   An adapter that does not advertise it ignores the field without an error, so it is never
//!   sent to one.
//! - **The first prompt of a new session** otherwise, queued as an automatic prompt: the text goes
//!   to the harness as the first message and stays in the session's history.
//!
//! A file in the project folder (`AGENTS.md`, `CLAUDE.md`) is not used: the harnesses read them,
//! but a workspace can be a folder the person already has with such a file, and Soyeht never writes
//! over the person's files for this. The text is read from the workspace's binding each time a
//! session opens, so a persona the person changed is the one the next session gets. It is never
//! written to a log.

use super::state::State;
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::PromptOrigin;
use soyeht_store::avatars::Persona;

/// The prompt id of the persona prompt of one pane, so an open that repeats queues it once.
const PROMPT_ID_PREFIX: &str = "avatar-persona-";

/// What a harness that takes the persona as its first message is told about that message.
const PROMPT_NOTE: &str = "These are your standing instructions for this conversation, not a \
                           request from the person: do not answer them.";

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

/// Queues the persona as the first prompt of a session that has no system prompt to carry it.
/// Nothing is queued twice, nor for an adapter that took it as the system prompt.
pub(crate) fn queue_first_prompt(
    state: &mut State,
    persona: Option<&str>,
    generation: u32,
    at: String,
) -> Result<()> {
    let Some(text) = persona.filter(|_| !accepts_system_prompt(state)) else {
        return Ok(());
    };
    let prompt_id = format!("{PROMPT_ID_PREFIX}{generation}");
    if state
        .queue
        .iter()
        .any(|queued| queued.prompt_id == prompt_id)
    {
        return Ok(());
    }
    state.enqueue_identified(
        prompt_id,
        format!("{text}\n\n{PROMPT_NOTE}"),
        PromptOrigin::Automatic,
        at,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
