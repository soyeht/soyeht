//! The engine's own build identity: the SHA-256 of its executable and its package version.

use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use soyeht_pane_state::EngineBuild;
use soyeht_platform::log;
use std::{fs::File, io::Read, sync::OnceLock, thread};

/// The build of one process, hashed at most once. Reading and hashing the whole executable
/// takes seconds on a loaded host, so it must neither run under the Core lock nor hold up the
/// engine's first listen. [`EngineBuildCell::ready`] never waits, so code under the lock asks
/// it and, when the hash is still running, moves the work off the lock; [`wait`] is for code
/// that holds no lock. A failure is kept like a success, so it is neither logged nor retried
/// again.
///
/// [`wait`]: EngineBuildCell::wait
pub struct EngineBuildCell {
    hashed: OnceLock<std::result::Result<EngineBuild, String>>,
    hash: fn() -> Result<EngineBuild>,
}

impl EngineBuildCell {
    pub const fn new(hash: fn() -> Result<EngineBuild>) -> Self {
        Self {
            hashed: OnceLock::new(),
            hash,
        }
    }

    /// The build or the reason it is unknown, or `None` while the hash is still running.
    pub fn ready(&self) -> Option<Result<EngineBuild>> {
        self.hashed.get().map(Self::answer)
    }

    /// The build, hashing it first if no caller has started to. Callers that come while it
    /// runs wait for that one result. A thread that holds the Core lock is refused, as
    /// `soyeht_process::run` refuses it, whether or not the hash happens to be done.
    pub fn wait(&self) -> Result<EngineBuild> {
        if let Some(holder) = soyeht_process::forbidden() {
            return Err(errors::internal(format!(
                "The engine build was awaited while the {holder} lock is held; blocking work runs off that lock"
            )));
        }
        Self::answer(self.hashed.get_or_init(|| match (self.hash)() {
            Ok(build) => Ok(build),
            Err(error) => {
                log::failure("engine.build_hash_failed", &error);
                Err(error.to_string())
            }
        }))
    }

    fn answer(hashed: &std::result::Result<EngineBuild, String>) -> Result<EngineBuild> {
        hashed.clone().map_err(errors::internal)
    }
}

static ENGINE_BUILD: EngineBuildCell = EngineBuildCell::new(hash_engine_executable);

/// The engine's own build identity, which every capture records.
pub fn cell() -> &'static EngineBuildCell {
    &ENGINE_BUILD
}

/// Starts hashing the engine's executable on its own thread, so the build is known by the
/// time a capture asks for it. A failure surfaces again, with its cause, in the capture that
/// needs the build.
pub fn prefetch_engine_build() {
    let started = thread::Builder::new()
        .name("engine-build".into())
        .spawn(|| cell().wait());
    if let Err(error) = started {
        log::failure("engine.build_prefetch_unavailable", &error);
    }
}

fn hash_engine_executable() -> Result<EngineBuild> {
    let executable = std::env::current_exe()?;
    let mut file = File::open(executable)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    let build = EngineBuild {
        package_version: env!("CARGO_PKG_VERSION").into(),
        executable_sha256: format!("{digest:x}"),
    };
    build.validate()?;
    Ok(build)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_build_is_hashed_once_and_every_caller_gets_the_same_identity() {
        prefetch_engine_build();
        let first = cell().wait().unwrap();
        let second = cell().wait().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.executable_sha256.len(), 64);
        assert_eq!(cell().ready().unwrap().unwrap(), first);
    }

    #[test]
    fn a_thread_holding_the_core_lock_cannot_wait_for_the_build() {
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        fn counted() -> Result<EngineBuild> {
            RUNS.fetch_add(1, Ordering::SeqCst);
            Err(errors::internal("never asked"))
        }
        let cell = EngineBuildCell::new(counted);
        let _held = soyeht_process::forbid("Core");
        let error = cell.wait().unwrap_err();
        assert!(error.to_string().contains("Core lock is held"), "{error}");
        assert_eq!(RUNS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_failed_hash_is_kept_and_not_run_again() {
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        fn failing() -> Result<EngineBuild> {
            RUNS.fetch_add(1, Ordering::SeqCst);
            Err(errors::internal("executable unreadable"))
        }
        let cell = EngineBuildCell::new(failing);
        assert!(cell.ready().is_none());
        for _ in 0..3 {
            let error = cell.wait().unwrap_err();
            assert!(
                error.to_string().contains("executable unreadable"),
                "{error}"
            );
        }
        assert!(cell.ready().unwrap().is_err());
        assert_eq!(RUNS.load(Ordering::SeqCst), 1);
    }
}
