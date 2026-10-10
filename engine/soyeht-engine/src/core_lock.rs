//! The only way to take the engine's Core lock (ADR 0079).
//!
//! The terminal path (output drain and stream pump) shares this lock with
//! every request, so it must only ever be held briefly. A guard marks its
//! thread: a bounded command started under it is refused
//! (`soyeht_process::forbid`), and a hold longer than [`SLOW_HOLD`] is logged
//! with the holder's name so a regression is visible in the engine log.

use crate::{
    core::Core,
    errors::{Error, Result},
};
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

pub type Shared = Arc<Mutex<Core>>;

/// A hold this long delays terminal output by more than a frame or two.
pub const SLOW_HOLD: Duration = Duration::from_millis(100);
const HOLDER: &str = "Core";

pub struct CoreGuard<'a> {
    core: MutexGuard<'a, Core>,
    _forbidden: soyeht_process::Forbidden,
    purpose: &'static str,
    acquired: Instant,
}

/// Locks Core for `purpose`, a short static name used in the slow-hold log.
/// Call it from a blocking thread, never from an async task.
pub fn lock<'a>(shared: &'a Shared, purpose: &'static str) -> Result<CoreGuard<'a>> {
    let core = shared.lock().map_err(|_| Error::EngineStatePoisoned)?;
    Ok(CoreGuard {
        core,
        _forbidden: soyeht_process::forbid(HOLDER),
        purpose,
        acquired: Instant::now(),
    })
}

/// Runs `operation` under the lock on a blocking thread, for async callers.
pub async fn with<T, F>(shared: &Shared, purpose: &'static str, operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Core) -> Result<T> + Send + 'static,
{
    let shared = Arc::clone(shared);
    tokio::task::spawn_blocking(move || {
        let mut core = lock(&shared, purpose)?;
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

impl Drop for CoreGuard<'_> {
    fn drop(&mut self) {
        let held = self.acquired.elapsed();
        if held >= SLOW_HOLD {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event":"engine.core_lock_slow",
                    "purpose":self.purpose,
                    "heldMs":held.as_millis(),
                })
            );
        }
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
}
