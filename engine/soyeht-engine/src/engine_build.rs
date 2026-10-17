//! The engine's own build identity: the SHA-256 of its executable and its package version.

use crate::errors::Result;
use sha2::{Digest, Sha256};
use soyeht_pane_state::EngineBuild;
use std::{
    fs::File,
    io::Read,
    sync::{Mutex, PoisonError},
    thread,
};

/// The engine build every capture records, hashed once per process. Reading and hashing the
/// whole executable takes seconds on a loaded host, so it must neither run under the Core
/// lock nor hold up the engine's first listen: [`prefetch_engine_build`] starts it at launch,
/// and a caller that comes while it runs waits for the one result instead of hashing again.
pub fn installed_engine_build() -> Result<EngineBuild> {
    static BUILD: Mutex<Option<EngineBuild>> = Mutex::new(None);
    let mut cached = BUILD.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(build) = cached.as_ref() {
        return Ok(build.clone());
    }
    let build = hash_engine_executable()?;
    *cached = Some(build.clone());
    Ok(build)
}

/// Starts hashing the engine's executable on its own thread, so the build is known by the
/// time a capture asks for it. A failure surfaces again, with its cause, in the capture that
/// needs the build.
pub fn prefetch_engine_build() {
    let started = thread::Builder::new()
        .name("engine-build".into())
        .spawn(|| installed_engine_build().map(|_| ()));
    if let Err(error) = started {
        soyeht_platform::log::failure("engine.build_prefetch_unavailable", &error);
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

    #[test]
    fn the_build_is_hashed_once_and_every_caller_gets_the_same_identity() {
        prefetch_engine_build();
        let first = installed_engine_build().unwrap();
        let second = installed_engine_build().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.executable_sha256.len(), 64);
    }
}
