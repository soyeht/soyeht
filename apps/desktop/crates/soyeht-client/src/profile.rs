//! Where the engine of the Dev profile listens.
//!
//! The directory layout comes from `contracts/profiles.json` through the generated
//! `soyeht_protocol::profile` constants. A client never invents its own location: the engine
//! keeps its socket there on macOS, Linux and Windows alike, so the default needs no setup.

use crate::{Error, Result};
use soyeht_protocol::profile::{DEV_DATA_DIRECTORY_FROM_HOME, DEV_ENGINE_SOCKET};
use std::path::PathBuf;

/// Overrides the profile location, for example to reach a disposable engine.
pub const SOCKET_ENV: &str = "SOYEHT_ENGINE_SOCKET";

/// The engine socket: the environment override, else the profile directory under the user's
/// home, which is where the engine itself keeps its socket on every platform.
pub fn engine_socket() -> Result<PathBuf> {
    resolve(
        std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        std::env::home_dir(),
    )
}

fn resolve(override_path: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path.filter(|path| !path.as_os_str().is_empty()) {
        return Ok(path);
    }
    let home = home.ok_or(Error::NoEngineSocket)?;
    Ok(home
        .join(DEV_DATA_DIRECTORY_FROM_HOME)
        .join(DEV_ENGINE_SOCKET))
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
    fn the_default_is_the_engine_socket_inside_the_profile_directory() {
        let expected = PathBuf::from("/home/a")
            .join(DEV_DATA_DIRECTORY_FROM_HOME)
            .join(DEV_ENGINE_SOCKET);
        assert_eq!(resolve(None, Some("/home/a".into())).unwrap(), expected);
        assert_eq!(
            resolve(Some(PathBuf::new()), Some("/home/a".into())).unwrap(),
            expected
        );
    }

    #[test]
    fn without_a_home_directory_there_is_no_default() {
        assert!(matches!(resolve(None, None), Err(Error::NoEngineSocket)));
    }
}
