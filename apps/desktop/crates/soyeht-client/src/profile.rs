//! Where the engine of the Dev profile listens.
//!
//! The directory layout comes from `contracts/profiles.json` through the generated
//! `soyeht_protocol::profile` constants. A client never invents its own location.

use crate::{Error, Result};
use soyeht_protocol::profile::{DEV_DATA_DIRECTORY_FROM_HOME, DEV_ENGINE_SOCKET};
use std::path::PathBuf;

/// Overrides the profile location, for example to reach a disposable engine.
pub const SOCKET_ENV: &str = "SOYEHT_ENGINE_SOCKET";

/// The engine socket: the environment override, else the profile directory of this platform.
pub fn engine_socket() -> Result<PathBuf> {
    resolve(
        std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn resolve(override_path: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path.filter(|path| !path.as_os_str().is_empty()) {
        return Ok(path);
    }
    // The profile contract defines the data directory for macOS only. The Linux and Windows
    // locations arrive with the engine port; until then an override is required there.
    if cfg!(target_os = "macos") {
        let home = home.ok_or(Error::NoEngineSocket)?;
        return Ok(home
            .join(DEV_DATA_DIRECTORY_FROM_HOME)
            .join(DEV_ENGINE_SOCKET));
    }
    Err(Error::NoEngineSocket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_over_the_profile_directory() {
        let path = resolve(
            Some("/run/soyeht/engine.sock".into()),
            Some("/home/a".into()),
        );
        assert_eq!(path.unwrap(), PathBuf::from("/run/soyeht/engine.sock"));
    }

    #[test]
    fn empty_override_is_ignored() {
        let path = resolve(Some(PathBuf::new()), Some("/Users/a".into()));
        assert_eq!(path.is_ok(), cfg!(target_os = "macos"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_the_generated_profile_layout() {
        let path = resolve(None, Some("/Users/a".into())).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/Users/a/Library/Application Support/Soyeht.next/dev/engine.sock")
        );
        assert!(resolve(None, None).is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_platforms_require_an_override_until_the_engine_port_defines_one() {
        assert!(matches!(
            resolve(None, Some("/home/a".into())),
            Err(Error::NoEngineSocket)
        ));
    }
}
