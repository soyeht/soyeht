//! Closing processes: one or many at once. The registry is written once for all of them, under the
//! state lock; the processes are then ended and waited for with the lock released (ADR 0171).

use super::{Live, State, error, lock, reach};
use crate::{
    Error, Result,
    child::{Child, Reach},
    wire::{MAX_CLOSE_MANY, Response},
    wire_kind,
};
use std::sync::{Arc, MutexGuard, Weak};

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
/// close is idempotent. One that another request is still ending is returned too, so this close
/// waits for it as well and does not answer before it is gone (a new engine's close, while the
/// connection it replaced is still closing). The processes come in the registry's order, which
/// is the order their locks are taken in, whatever the request.
fn mark_closed(state: &mut State, instance_ids: &[String]) -> Result<Vec<Arc<Live>>> {
    let mut newly_closed: Vec<String> = Vec::new();
    let mut known: Vec<String> = Vec::new();
    for instance in state.registry.instances.values_mut() {
        if !instance_ids.contains(&instance.instance_id) {
            continue;
        }
        known.push(instance.instance_id.clone());
        if !instance.closed {
            instance.closed = true;
            newly_closed.push(instance.spawn.instance_key.clone());
        }
    }
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
    let mut lives = Vec::with_capacity(known.len());
    for instance_id in known {
        if let Some(live) = state.live.remove(&instance_id) {
            state.closing.insert(instance_id, Arc::downgrade(&live));
            lives.push(live);
        } else if let Some(live) = state.closing.get(&instance_id).and_then(Weak::upgrade) {
            lives.push(live);
        }
    }
    // What nothing holds any more is over.
    state.closing.retain(|_, live| live.strong_count() > 0);
    Ok(lives)
}

/// Ends every process still running, together, and announces their exits. A process that cannot
/// be looked at is ended as if it ran, and the others are ended whatever fails: the first
/// failure is returned after all of them.
fn end_all(lives: Vec<Arc<Live>>) -> Result<()> {
    let mut failure: Option<Error> = None;
    let mut guards = Vec::with_capacity(lives.len());
    for live in &lives {
        match lock(&live.child, "child") {
            Ok(guard) => guards.push((guard, live)),
            Err(error) => {
                failure.get_or_insert(error);
            }
        }
    }
    let mut running: Vec<(&mut Child, Reach)> = Vec::new();
    for (guard, live) in &mut guards {
        match guard.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => running.push((&mut **guard, reach(live))),
            Err(error) => {
                failure.get_or_insert(error.into());
                running.push((&mut **guard, reach(live)));
            }
        }
    }
    Child::end_together(&mut running);
    drop(running);
    drop(guards);
    for live in &lives {
        live.mark_exited();
    }
    failure.map_or(Ok(()), Err)
}

/// Records the exit status of the process under `key`. True when it is worth writing the registry:
/// a closed process is not listed and a restart marks every process closed, so its status is
/// not worth a durable write of the whole registry under the state lock, which closing many
/// processes would repeat for each (ADR 0171).
pub(super) fn record_exit(state: &mut State, key: &str, code: i32) -> bool {
    state
        .registry
        .instances
        .get_mut(key)
        .is_some_and(|instance| {
            instance.exit_code = Some(code);
            !instance.closed
        })
}
