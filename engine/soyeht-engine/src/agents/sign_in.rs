//! An agent that waits for sign-in keeps the person's prompts. Every prompt is
//! written to the transcript with a notice that explains the wait, and a new
//! prompt makes the engine check the sign-in again, so a login repaired outside
//! Soyeht recovers without closing the pane.

use super::{
    Changes,
    purpose::Purpose,
    state::{Phase, State},
    transcript,
};
use crate::{errors::Result, model::StoredPane};
use serde_json::json;
use soyeht_protocol::{PromptItem, PromptOrigin, TranscriptKind, TranscriptRole, event};

/// What the transcript says once, after the prompts it could not start.
pub(super) const WAITING_NOTICE: &str = "Sign-in is required before the agent can run this. Your prompt stays queued and runs once the agent is signed in. Sending another prompt checks the sign-in again.";

/// The agent asked for sign-in: the pane waits for the person.
pub(super) fn needs_login(
    state: &mut State,
    saved: &mut StoredPane,
    changes: &mut Changes,
) -> Result<()> {
    state.phase = Phase::NeedsLogin;
    state.auth_required = true;
    state.probe_started_ms = None;
    saved.pane.state = soyeht_protocol::PaneState::Starting;
    saved.pane.phrase = "Sign-in required".into();
    let methods = state
        .initialized
        .as_ref()
        .map(|i| {
            i.auth_methods
                .iter()
                .map(|m| m.public.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    changes.events.push((
        event::PANE_NEEDS_LOGIN.into(),
        json!({"paneId":saved.pane.pane_id,"harness":state.launch.id,"authMethods":methods}),
    ));
    record_waiting_prompts(state, saved, changes)
}

/// A prompt arrived at a pane that may be waiting for sign-in. When no sign-in
/// is in progress the agent restarts and asks again: the person may have
/// signed in outside Soyeht. Otherwise the prompt is recorded as waiting.
pub(super) fn prompt_arrived(
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
    origin: &PromptOrigin,
) -> Result<()> {
    // Only the person's prompts are recorded and only they check the sign-in
    // again; an automatic prompt waits in the queue without restarting the agent.
    if state.phase != Phase::NeedsLogin || *origin != PromptOrigin::Human {
        return Ok(());
    }
    if signing_in(state) {
        return record_waiting_prompts(state, saved, changes);
    }
    state.phase = Phase::RestartAfterProbe;
    Ok(())
}

/// Whether a sign-in terminal or a protocol sign-in is still running.
fn signing_in(state: &State) -> bool {
    state.login_pane_id.is_some()
        || state.unconfirmed_login_pane_id.is_some()
        || state
            .client
            .pending
            .values()
            .any(|call| Purpose::parse(&call.context) == Purpose::Authenticate)
}

/// Writes each queued prompt of the person that the transcript does not hold
/// yet, then the notice that says why none of them runs.
fn record_waiting_prompts(
    state: &mut State,
    saved: &StoredPane,
    changes: &mut Changes,
) -> Result<()> {
    let waiting: Vec<_> = state
        .queue
        .iter()
        .filter(|prompt| {
            prompt.origin == PromptOrigin::Human
                && !state.transcribed_prompts.contains(&prompt.prompt_id)
        })
        .cloned()
        .collect();
    if waiting.is_empty() {
        return Ok(());
    }
    let generation = saved.pane.generation;
    for prompt in &waiting {
        transcript::user_prompt(state, generation, prompt, changes)?;
        state.transcribed_prompts.insert(prompt.prompt_id.clone());
    }
    let notice = transcript::new_message(
        state,
        generation,
        TranscriptRole::System,
        TranscriptKind::System,
        WAITING_NOTICE.into(),
        true,
    )?;
    transcript::put(changes, &saved.pane.pane_id, notice, "system_message")
}

/// A queued prompt starts: the transcript gets it now, unless it was written
/// while it waited.
pub(super) fn record_started(
    state: &mut State,
    generation: u32,
    prompt: &PromptItem,
    changes: &mut Changes,
) -> Result<()> {
    if state.transcribed_prompts.remove(&prompt.prompt_id) {
        return Ok(());
    }
    transcript::user_prompt(state, generation, prompt, changes)
}
