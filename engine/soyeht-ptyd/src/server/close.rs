//! Closing processes: one or many at once. The registry is written once for all of them, under the
//! state lock; the processes are then ended and waited for with the lock released (ADR 0171).

use super::{Live, State, error, reach};
use crate::{
    Error, Result,
    child::{Child, Reach},
    wire::{MAX_CLOSE_MANY, Response},
    wire_kind,
};
use soyeht_platform::log;
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

/// Ends every process still running, together, and announces their exits. A poisoned lock is
/// recovered and its process ended: a panic elsewhere says nothing about the process. A process
/// that cannot be looked at (`try_wait` fails, which on Unix means it is not our child any more
/// and its pid may belong to another process) is never signalled; the failure is returned after
/// the others are ended.
fn end_all(lives: Vec<Arc<Live>>) -> Result<()> {
    let mut failure: Option<Error> = None;
    let mut guards: Vec<_> = lives
        .iter()
        .map(|live| {
            let guard = live.child.lock().unwrap_or_else(|poisoned| {
                log::event("ptyd.child_lock_poisoned", &[]);
                poisoned.into_inner()
            });
            (guard, live)
        })
        .collect();
    let mut running: Vec<(&mut Child, Reach)> = Vec::new();
    for (guard, live) in &mut guards {
        match guard.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => running.push((&mut **guard, reach(live))),
            Err(error) => {
                failure.get_or_insert(error.into());
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        registry::Registry,
        retention::Retention,
        server::{OutputState, watch::Watchers},
        wire::{Instance, ProcessKind, Spawn},
    };
    use std::sync::{Mutex, atomic::AtomicBool};

    fn state_with_one_process() -> (State, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!("soyeht-close-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let spawn = Spawn {
            instance_key: "key".into(),
            generation: 1,
            kind: ProcessKind::Pty,
            cwd: "/".into(),
            argv: vec!["sleep".into()],
            env: Default::default(),
            cols: 80,
            rows: 24,
        };
        let instance = Instance {
            instance_id: "one".into(),
            supervisor_id: "s".into(),
            spawn,
            pid: 1,
            closed: false,
            exit_code: None,
        };
        let live = Live {
            master: Mutex::new(std::fs::File::open("/dev/null").unwrap()),
            resizer: None,
            child: Mutex::new(Child::new(
                std::process::Command::new("sleep")
                    .arg("60")
                    .spawn()
                    .unwrap(),
            )),
            output: Mutex::new(OutputState {
                retention: Retention::new(1024).unwrap(),
                pending: None,
                told: false,
            }),
            guard: None,
            exited: AtomicBool::new(false),
            read_done: AtomicBool::new(false),
            instance_id: "one".into(),
            watchers: Arc::default(),
            exit_announced: AtomicBool::new(false),
            foreground_unreadable: AtomicBool::new(false),
        };
        let state = State {
            directory: directory.clone(),
            supervisor_id: "s".into(),
            registry: Registry {
                version: 1,
                engine_generation: 0,
                supervisor_generation: 1,
                instances: [("key".to_owned(), instance)].into(),
            },
            live: [("one".to_owned(), Arc::new(live))].into(),
            closing: Default::default(),
            active: None,
            inputs: Default::default(),
            watchers: Arc::<Watchers>::default(),
        };
        (state, directory)
    }

    #[test]
    fn a_second_close_gets_the_process_the_first_is_still_ending_and_nothing_after() {
        let (mut state, directory) = state_with_one_process();
        let ids = ["one".to_owned()];

        let first = mark_closed(&mut state, &ids).unwrap();
        assert_eq!(first.len(), 1);
        let second = mark_closed(&mut state, &ids).unwrap();
        assert_eq!(second.len(), 1, "the process is still being ended");
        assert!(Arc::ptr_eq(&first[0], &second[0]));

        // The close that held it is over, and so is the process.
        end_all(first).unwrap();
        drop(second);
        assert!(mark_closed(&mut state, &ids).unwrap().is_empty());
        assert!(state.closing.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
