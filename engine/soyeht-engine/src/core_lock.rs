//! The only way to take the engine's Core lock (ADR 0079).
//!
//! The terminal path (output drain and stream pump) shares this lock with
//! every request, so it must only ever be held briefly. A guard marks its
//! thread: a bounded command started under it is refused
//! (`soyeht_process::forbid`), and a hold longer than [`crate::slow_holds::SLOW`] is logged
//! with the holder's name so a regression is visible in the engine log. Every hold is also counted
//! (`lock_usage`), whether it was slow or not. Holds
//! that repeat are logged by the rule of `slow_holds`: the first, a much longer
//! one, and a summary each minute.

use crate::{
    core::Core,
    errors::{Error, Result},
    lock_usage::Observation,
};
use soyeht_platform::hold_cost;
use std::{
    cell::Cell,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

pub type Shared = Arc<Mutex<Core>>;

const HOLDER: &str = "Core";

pub struct CoreGuard<'a> {
    core: MutexGuard<'a, Core>,
    _forbidden: soyeht_process::Forbidden,
    /// Last, so the lock is already released when a slow hold is logged.
    _hold: Hold,
}

struct Hold {
    purpose: &'static str,
    acquired: Instant,
    /// From the request for the lock to the moment it was taken.
    waited: Duration,
}

thread_local! {
    /// The request method a `dispatch` hold is for, set by the handler once it knows it.
    static METHOD: Cell<&'static str> = const { Cell::new("") };
}

/// Names the request this hold serves, so its time is told apart from the other requests'.
pub fn name_hold(method: &'static str) {
    METHOD.with(|name| name.set(method));
}

/// Locks Core for `purpose`, a short static name used in the slow-hold log.
/// Call it from a blocking thread, never from an async task.
pub fn lock<'a>(shared: &'a Shared, purpose: &'static str) -> Result<CoreGuard<'a>> {
    lock_requested(shared, purpose, Instant::now())
}

/// Locks Core for a request made at `requested`, so the time the request waited before it asked the
/// mutex (the queue of the blocking pool) is part of its wait.
fn lock_requested<'a>(
    shared: &'a Shared,
    purpose: &'static str,
    requested: Instant,
) -> Result<CoreGuard<'a>> {
    let core = shared.lock().map_err(|_| Error::EngineStatePoisoned)?;
    let acquired = Instant::now();
    hold_cost::start();
    METHOD.with(|name| name.set(""));
    Ok(CoreGuard {
        core,
        _forbidden: soyeht_process::forbid(HOLDER),
        _hold: Hold {
            purpose,
            acquired,
            waited: acquired.saturating_duration_since(requested),
        },
    })
}

/// Runs `operation` under the lock on a blocking thread, for async callers.
pub async fn with<T, F>(shared: &Shared, purpose: &'static str, operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Core) -> Result<T> + Send + 'static,
{
    let shared = Arc::clone(shared);
    let requested = Instant::now();
    tokio::task::spawn_blocking(move || {
        let mut core = lock_requested(&shared, purpose, requested)?;
        operation(&mut core)
    })
    .await
    .map_err(Error::Join)?
}

impl Deref for CoreGuard<'_> {
    type Target = Core;

    fn deref(&self) -> &Core {
        &self.core
    }
}

impl DerefMut for CoreGuard<'_> {
    fn deref_mut(&mut self) -> &mut Core {
        &mut self.core
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        crate::slow_holds::note(&Observation {
            purpose: self.purpose,
            method: METHOD.with(|name| name.replace("")),
            held: self.acquired.elapsed(),
            waited: self.waited,
            costs: hold_cost::finish(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_under_the_core_lock_is_refused_and_allowed_after_it() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-core-lock-{}", uuid::Uuid::new_v4()));
        let shared = Arc::new(Mutex::new(Core::open(&directory).unwrap()));
        // The test executable lists its tests and ends: a command on every system.
        let command = || {
            let mut list = std::process::Command::new(std::env::current_exe().unwrap());
            list.arg("--list");
            soyeht_process::run(
                list,
                soyeht_process::Limits::new(Duration::from_secs(10), 1 << 20),
            )
        };
        {
            let _core = lock(&shared, "test").unwrap();
            assert!(matches!(
                command(),
                Err(soyeht_process::Error::UnderLock { holder: "Core", .. })
            ));
        }
        assert!(command().unwrap().success());
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_hold_says_how_its_time_split_between_statements_and_the_file_port() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-core-cost-{}", uuid::Uuid::new_v4()));
        let shared = Arc::new(Mutex::new(Core::open(&directory).unwrap()));
        let pause = Duration::from_millis(30);
        {
            let mut core = lock(&shared, "cost_split_test").unwrap();
            hold_cost::timed(hold_cost::Cost::File, || std::thread::sleep(pause));
            core.store.workspaces().unwrap();
            core.store.prune().unwrap();
        }
        let costs = crate::slow_holds::costs_of("cost_split_test").unwrap();
        assert!(costs.file >= pause, "{costs:?}");
        assert!(costs.sql > Duration::ZERO, "{costs:?}");
        assert!(costs.commit > Duration::ZERO, "{costs:?}");
        assert_eq!(costs.segment, Duration::ZERO, "{costs:?}");
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
