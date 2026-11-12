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
//!
//! A lock that a panic poisoned is not recovered: the state it guards may be half changed, and
//! every request would go on failing. The first caller that finds it logs the fact and ends the
//! process through [`crate::hooks::Hooks::end_process`], so the service manager or the window
//! starts a new engine (ADR 0159).

use crate::{
    core::Core,
    errors::{Error, Result},
    hooks::Hooks,
    lock_usage::Observation,
};
use soyeht_platform::{hold_cost, log};
use std::{
    cell::Cell,
    ops::{Deref, DerefMut},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub type Shared = Arc<Mutex<Core>>;

const HOLDER: &str = "Core";

/// The exit code of an engine that found its Core lock poisoned (`EX_SOFTWARE` of sysexits), apart
/// from the 1 of a start that failed, so the log and the service manager's history tell them apart.
/// Every service definition restarts a program that ends with a failure code (contract 13).
pub const POISONED_EXIT_CODE: i32 = 70;

/// Set by the first caller that finds the lock poisoned: only that one logs and ends the process.
static POISON_HANDLED: AtomicBool = AtomicBool::new(false);

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
    /// Raised when the hold ends, so the tick looks at what it may have changed. The tick's own
    /// holds do not raise it.
    kick: Option<Arc<tokio::sync::Notify>>,
    /// The hooks, when they measure holds (a test engine).
    watcher: Option<Arc<dyn Hooks>>,
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
    let core = shared
        .lock()
        .map_err(|poisoned| poisoned_core(&poisoned.get_ref().hooks))?;
    let acquired = Instant::now();
    hold_cost::start();
    METHOD.with(|name| name.set(""));
    let kick = (!crate::tick::OWN_HOLDS.contains(&purpose)).then(|| core.background.kicker());
    let watcher = core.hooks.watches_holds().then(|| Arc::clone(&core.hooks));
    Ok(CoreGuard {
        core,
        _forbidden: soyeht_process::forbid(HOLDER),
        _hold: Hold {
            purpose,
            acquired,
            waited: acquired.saturating_duration_since(requested),
            kick,
            watcher,
        },
    })
}

/// What a caller does on finding the Core lock poisoned: log the fact once (its kind, never the
/// panic's message), end the process, and fail the request if the process is still there.
fn poisoned_core(hooks: &Arc<dyn Hooks>) -> Error {
    let error = Error::EngineStatePoisoned;
    if !POISON_HANDLED.swap(true, Ordering::SeqCst) {
        log::failure("engine.core_poisoned", &error);
        hooks.end_process(POISONED_EXIT_CODE);
    }
    error
}

/// Runs `operation` under the lock on a blocking thread, for async callers.
///
/// A hold raises the tick when it ends, unless `purpose` is in `tick::OWN_HOLDS`: a hold the tick
/// takes itself must be listed there by name, or its end would run the tick again, for ever.
/// (`tick`'s tests check that every purpose the tick uses is.)
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
        let observation = Observation {
            purpose: self.purpose,
            method: METHOD.with(|name| name.replace("")),
            held: self.acquired.elapsed(),
            waited: self.waited,
            costs: hold_cost::finish(),
        };
        if let Some(watcher) = &self.watcher {
            watcher.lock_held(
                observation.purpose,
                observation.method,
                observation.held,
                observation.waited,
            );
        }
        crate::slow_holds::note(&observation);
        if let Some(kick) = &self.kick {
            kick.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;

    /// Records the exit the engine asks for instead of ending the test process.
    #[derive(Default)]
    struct RecordedExit {
        calls: AtomicI32,
        code: AtomicI32,
    }

    impl Hooks for RecordedExit {
        fn end_process(&self, code: i32) {
            self.code.store(code, Ordering::SeqCst);
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn a_hold_that_is_not_the_ticks_raises_the_tick_and_the_ticks_own_does_not() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-core-kick-{}", uuid::Uuid::new_v4()));
        let shared = Arc::new(Mutex::new(Core::open(&directory).unwrap()));
        let kick = lock(&shared, "setup").unwrap().background.kicker();
        let raised = |wait: Duration| {
            let kick = Arc::clone(&kick);
            async move { tokio::time::timeout(wait, kick.notified()).await.is_ok() }
        };
        // The setup hold above raised it, and so may a job the engine started when it opened:
        // wait for quiet before asking.
        assert!(raised(Duration::from_secs(5)).await);
        while raised(Duration::from_millis(300)).await {}

        for own in crate::tick::OWN_HOLDS {
            drop(lock(&shared, own).unwrap());
        }
        assert!(
            !raised(Duration::from_millis(100)).await,
            "the tick's own holds do not raise it"
        );
        drop(lock(&shared, "dispatch").unwrap());
        assert!(
            raised(Duration::from_secs(5)).await,
            "a request's hold does"
        );
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The only test that poisons a lock: `POISON_HANDLED` is one flag for the process.
    #[test]
    fn a_panic_under_the_core_lock_ends_the_engine_once_with_its_own_code() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-core-poison-{}", uuid::Uuid::new_v4()));
        let exit = Arc::new(RecordedExit::default());
        let shared = Arc::new(Mutex::new(
            Core::open_with(&directory, Arc::clone(&exit) as Arc<dyn Hooks>).unwrap(),
        ));
        drop(lock(&shared, "healthy").unwrap());
        assert_eq!(exit.calls.load(Ordering::SeqCst), 0);

        let panicking = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            let _core = lock(&panicking, "poisoning").unwrap();
            panic!("a bug under the lock");
        });
        assert!(thread.join().is_err());

        assert!(matches!(
            lock(&shared, "after_panic"),
            Err(Error::EngineStatePoisoned)
        ));
        assert_eq!(exit.calls.load(Ordering::SeqCst), 1);
        assert_eq!(exit.code.load(Ordering::SeqCst), POISONED_EXIT_CODE);
        // Callers that arrive while the process is ending fail without ending it again.
        assert!(lock(&shared, "second").is_err());
        assert_eq!(exit.calls.load(Ordering::SeqCst), 1);
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

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
