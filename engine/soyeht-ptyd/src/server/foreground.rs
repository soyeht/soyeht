//! `Foreground`: what each terminal the supervisor holds runs in its foreground (ADR 0165).

use super::{State, error, lock};
use crate::{
    Result,
    wire::{Foreground, MAX_FOREGROUND, Response},
    wire_kind,
};
use soyeht_platform::log;
use std::sync::atomic::Ordering;

/// The programs of each of `instance_ids` that is a live terminal; an id the supervisor does not
/// know, or that is not a terminal, is left out. A terminal whose process ended answers with no
/// programs. The caller holds the state, and each answer is a few system reads.
pub(super) fn execute_foreground(state: &State, instance_ids: &[String]) -> Result<Response> {
    if instance_ids.len() > MAX_FOREGROUND {
        return Ok(error(wire_kind::INVALID_PARAMS, "too many terminals"));
    }
    let mut answers = Vec::with_capacity(instance_ids.len());
    for instance_id in instance_ids {
        let Some(live) = state.live.get(instance_id) else {
            continue;
        };
        let Some(terminal) = &live.resizer else {
            continue;
        };
        let programs = if live.exited.load(Ordering::Acquire) {
            Vec::new()
        } else {
            let child = lock(&live.child, "child")?;
            match terminal.foreground(&child) {
                Ok(programs) => programs.into_iter().map(Into::into).collect(),
                Err(failure) => {
                    log::failure("ptyd.foreground_unreadable", &failure);
                    continue;
                }
            }
        };
        answers.push(Foreground {
            instance_id: instance_id.clone(),
            programs,
        });
    }
    Ok(Response::Foregrounds(answers))
}
