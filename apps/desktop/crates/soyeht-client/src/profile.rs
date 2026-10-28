//! Where the engine of the client's profile listens, and where the client keeps its log.
//!
//! The profile and its directory layout come from `contracts/profiles.json` through the
//! generated `soyeht_protocol::profile` constants. A client never invents its own location:
//! the engine keeps its socket in the profile's data directory in the user's native location
//! (`soyeht_platform::dirs`) on macOS, Linux and Windows alike, so the default needs no setup.

use crate::Result;
use soyeht_protocol::{
    data_directory::{data_directory, selected_profile},
    profile::{DESKTOP_LOG, ENGINE_SOCKET, Profile, SELECTION_ENV},
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Overrides the profile location, for example to reach a disposable engine.
pub const SOCKET_ENV: &str = "SOYEHT_ENGINE_SOCKET";

/// The profile this client runs as: `SOYEHT_PROFILE`, else the profile of the build, else Dev.
pub fn selected() -> Result<&'static Profile> {
    Ok(selected_profile(
        std::env::var_os(SELECTION_ENV).as_deref(),
    )?)
}

/// The data directory of [`selected`], where the engine keeps its socket and logs.
pub fn data_directory_of_profile() -> Result<PathBuf> {
    Ok(data_directory(
        selected()?,
        soyeht_platform::dirs::data_base(),
    )?)
}

/// The engine socket: the environment override, else the socket in the profile's data directory,
/// which is where the engine itself keeps it on every platform.
pub fn engine_socket() -> Result<PathBuf> {
    resolve(std::env::var_os(SOCKET_ENV), data_directory_of_profile())
}

/// The socket the environment names, when it names one: an empty value names nothing. A window
/// that finds one leaves the engine alone.
pub(crate) fn socket_override(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|path| !path.is_empty()).map(PathBuf::from)
}

/// The folder of a profile's data directory that holds the logs. The engine's launch agent writes
/// `engine.log` there, and the window starts the engine with its output going there too.
pub(crate) fn logs_directory(data_directory: &Path) -> PathBuf {
    soyeht_platform::process::detached::logs_directory(data_directory)
}

/// The file the client's log lines are also written to, so that a program started from a menu or
/// an icon (no terminal; on Windows no console) still leaves them somewhere. It lives in the
/// profile's data directory, on every platform.
pub fn log_file() -> Result<PathBuf> {
    Ok(log_file_in(&data_directory_of_profile()?))
}

fn log_file_in(directory: &Path) -> PathBuf {
    logs_directory(directory).join(DESKTOP_LOG)
}

fn resolve(override_value: Option<OsString>, directory: Result<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = socket_override(override_value) {
        return Ok(path);
    }
    Ok(directory?.join(ENGINE_SOCKET))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use soyeht_protocol::profile::DEV;

    fn dev_directory() -> Result<PathBuf> {
        data_directory(&DEV, Some("/data".into())).map_err(Error::from)
    }

    #[test]
    fn override_wins_over_the_profile_directory() {
        let path = resolve(Some("/run/soyeht/engine.sock".into()), dev_directory());
        assert_eq!(path.unwrap(), PathBuf::from("/run/soyeht/engine.sock"));
    }

    #[test]
    fn the_default_is_the_engine_socket_inside_the_profile_directory() {
        let expected = PathBuf::from("/data")
            .join(DEV.data_directory.native())
            .join(ENGINE_SOCKET);
        assert_eq!(resolve(None, dev_directory()).unwrap(), expected);
        assert_eq!(
            resolve(Some(OsString::new()), dev_directory()).unwrap(),
            expected
        );
    }

    #[test]
    fn the_log_file_sits_beside_the_engine_log_in_the_profile_directory() {
        let expected = PathBuf::from("/data")
            .join(DEV.data_directory.native())
            .join("logs")
            .join(DESKTOP_LOG);
        assert_eq!(log_file_in(&dev_directory().unwrap()), expected);
    }

    #[test]
    fn without_a_base_directory_there_is_no_default() {
        let missing = data_directory(&DEV, None).map_err(Error::from);
        assert!(matches!(
            resolve(None, missing),
            Err(Error::Protocol(soyeht_protocol::Error::HomeMissing))
        ));
    }
}
