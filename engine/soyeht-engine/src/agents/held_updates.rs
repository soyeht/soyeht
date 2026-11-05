//! The `session/update` notifications that arrive before `session/new` has named their session
//! (contract 04 §7).

use super::{
    Changes,
    state::{Phase, State},
    updates,
};
use crate::{
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::Value;

/// How many updates the engine holds for a session that `session/new` has not named yet. An
/// agent announces its commands and options, a handful of updates, before it answers.
const HELD_UPDATES_MAX: usize = 32;

/// An update with no session to match it against. Adapters announce their commands without
/// waiting for the answer to a request, so it is not a fault of the agent:
///
/// - while the pane's own `session/new` is open, the update is held until the answer names the
///   session (`apply_held`);
/// - before the probe's session exists, and after the probe, when the process is about to be
///   replaced, it belongs to a session the pane never shows, so it is ignored;
/// - in any other phase a session must exist, and its absence is an engine fault.
pub(super) fn without_session(state: &mut State, params: &Value) -> Result<bool> {
    match state.phase {
        Phase::Opening => {
            if state.held_updates.len() < HELD_UPDATES_MAX {
                state.held_updates.push(params.clone());
            } else if !state.held_overflowed {
                // Once for each open, not once for each update: an agent that floods the open
                // would otherwise fill the log.
                state.held_overflowed = true;
                soyeht_platform::log::event("engine.agent_updates_held_overflow", &[]);
            }
            Ok(false)
        }
        Phase::Initializing | Phase::ProbeNew | Phase::RestartAfterProbe | Phase::NeedsLogin => {
            Ok(false)
        }
        _ => Err(errors::invalid("Update arrived before a session identity")),
    }
}

/// Applies the updates held while the session was opening, now that the answer has named it. An
/// update for another session than the one opened is dropped.
pub(crate) fn apply_held(
    store: &impl StorePort,
    pane: &str,
    generation: u32,
    state: &mut State,
    timestamp: &str,
    changes: &mut Changes,
) -> Result<()> {
    let held = std::mem::take(&mut state.held_updates);
    state.held_overflowed = false;
    for params in held {
        if params["sessionId"].as_str() == state.session_id.as_deref() {
            updates::update(store, pane, generation, state, &params, timestamp, changes)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(phase: Phase) -> State {
        let mut state = super::super::idle::tests::ready_state();
        state.phase = phase;
        state.session_id = None;
        state
    }

    fn announcement() -> Value {
        json!({"sessionId":"new-session","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[]}})
    }

    #[test]
    fn an_update_while_the_session_opens_is_held_up_to_a_bound() {
        let mut opening = state(Phase::Opening);
        for _ in 0..HELD_UPDATES_MAX + 8 {
            assert!(!without_session(&mut opening, &announcement()).unwrap());
        }
        assert_eq!(opening.held_updates.len(), HELD_UPDATES_MAX);
    }

    #[test]
    fn an_update_of_a_session_nobody_shows_is_ignored() {
        for phase in [
            Phase::Initializing,
            Phase::ProbeNew,
            Phase::RestartAfterProbe,
            Phase::NeedsLogin,
        ] {
            let mut other = state(phase);
            assert!(!without_session(&mut other, &announcement()).unwrap());
            assert!(other.held_updates.is_empty());
        }
    }

    #[test]
    fn an_update_with_no_session_in_a_phase_that_has_one_is_a_fault() {
        let mut ready = state(Phase::Ready);
        assert!(without_session(&mut ready, &announcement()).is_err());
    }

    fn commands(session: &str, name: &str) -> Value {
        json!({"sessionId":session,"update":{"sessionUpdate":"available_commands_update",
            "availableCommands":[{"name":name,"description":"d"}]}})
    }

    fn commands_changed(changes: &Changes) -> usize {
        changes
            .events
            .iter()
            .filter(|(name, _)| name == soyeht_protocol::event::PANE_COMMANDS_CHANGED)
            .count()
    }

    #[test]
    fn held_updates_are_applied_in_order_and_the_ones_of_another_session_are_dropped() {
        let (store, directory) = crate::agents::state::tests::scratch_store();
        let mut opened = state(Phase::Opening);
        for params in [
            commands("new-session", "first"),
            commands("new-session", "second"),
            commands("another-session", "foreign"),
        ] {
            assert!(!without_session(&mut opened, &params).unwrap());
        }
        // The answer names the session and the pane is ready.
        opened.session_id = Some("new-session".into());
        opened.phase = Phase::Ready;
        let mut changes = Changes::default();
        apply_held(&store, "pane", 1, &mut opened, "now", &mut changes).unwrap();
        // The last one applied is the second of the session, and one change was announced for
        // each of the two applied: the first, then the second. The foreign one changed nothing.
        let names: Vec<_> = opened.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["second"]);
        assert_eq!(commands_changed(&changes), 2);
        assert!(opened.held_updates.is_empty());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_full_hold_is_marked_overflowed_and_a_release_starts_over() {
        let mut opening = state(Phase::Opening);
        for _ in 0..HELD_UPDATES_MAX + 3 {
            without_session(&mut opening, &announcement()).unwrap();
        }
        assert!(opening.held_overflowed);
        opening.release_held_updates();
        assert!(opening.held_updates.is_empty() && !opening.held_overflowed);
    }
}
