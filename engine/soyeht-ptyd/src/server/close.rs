//! Closing processes: one or many at once. The registry is written once for all of them, under the
//! state lock; the processes are then ended and waited for with the lock released (ADR 0171).

use super::{Live, State, error, lock, reach};
use crate::{
    Error, Result,
    child::{Child, Reach},
    wire::{MAX_CLOSE_MANY, Response},
    wire_kind,
};
use std::sync::{Arc, MutexGuard};

/// Answers a `CloseMany`: refused when it names too many, else [`close_answer`].
pub(super) fn close_many_answer(state: MutexGuard<'_, State>, instance_ids: &[String]) -> Response {
    if instance_ids.len() > MAX_CLOSE_MANY {
        return error(wire_kind::INVALID_PARAMS, "too many processes to close");
    }
    close_answer(state, instance_ids)
}

/// Closes `instance_ids` and answers when every process is gone. The registry is written once
/// for all of them and the wait happens with the state released.
pub(super) fn close_answer(mut state: MutexGuard<'_, State>, instance_ids: &[String]) -> Response {
    let closing = mark_closed(&mut state, instance_ids);
    drop(state);
    closing.and_then(end_all).map_or_else(
        |e| error(wire_kind::OPERATION_FAILED, &e.to_string()),
        |()| Response::Ok,
    )
}

/// Marks the processes closed in the registry, writes it once, and takes them out of the live
/// set. A process the registry does not know, or that was closed already, is not an error: a
/// close is idempotent.
fn mark_closed(state: &mut State, instance_ids: &[String]) -> Result<Vec<Arc<Live>>> {
    let mut newly_closed: Vec<String> = Vec::new();
    let mut known: Vec<&str> = Vec::new();
    for instance in state.registry.instances.values_mut() {
        if !instance_ids.contains(&instance.instance_id) {
            continue;
        }
        known.push(&instance.instance_id);
        if !instance.closed {
            instance.closed = true;
            newly_closed.push(instance.spawn.instance_key.clone());
        }
    }
    let known: Vec<String> = known.into_iter().map(str::to_owned).collect();
    if !newly_closed.is_empty()
        && let Err(failure) = state.registry.save(&state.directory)
    {
        for key in &newly_closed {
            state
                .registry
                .instances
                .get_mut(key)
                .ok_or(Error::RegistryInstanceMissing)?
                .closed = false;
        }
        return Err(failure);
    }
    Ok(known
        .iter()
        .filter_map(|instance_id| state.live.remove(instance_id))
        .collect())
}

/// Ends every process still running, together, and announces their exits.
fn end_all(lives: Vec<Arc<Live>>) -> Result<()> {
    let mut guards = Vec::with_capacity(lives.len());
    for live in &lives {
        guards.push(lock(&live.child, "child")?);
    }
    let mut running: Vec<(&mut Child, Reach)> = Vec::new();
    for (guard, live) in guards.iter_mut().zip(&lives) {
        if guard.try_wait()?.is_none() {
            running.push((&mut **guard, reach(live)));
        }
    }
    Child::end_together(&mut running);
    drop(running);
    drop(guards);
    for live in &lives {
        live.mark_exited();
    }
    Ok(())
}
