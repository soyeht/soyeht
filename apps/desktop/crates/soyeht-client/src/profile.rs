//! Where the engine of the Dev profile listens, and where the client keeps its log.
//!
//! The directory layout comes from `contracts/profiles.json` through the generated
//! `soyeht_protocol::profile` constants. A client never invents its own location: the engine
//! keeps its socket there on macOS, Linux and Windows alike, so the default needs no setup.

use crate::Result;
use soyeht_protocol::{data_directory::dev_data_directory, profile::DEV_ENGINE_SOCKET};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Overrides the profile location, for example to reach a disposable engine.
pub const SOCKET_ENV: &str = "SOYEHT_ENGINE_SOCKET";

/// The engine socket: the environment override, else the profile directory under the user's
/// home, which is where the engine itself keeps its socket on every platform.
pub fn engine_socket() -> Result<PathBuf> {
    resolve(std::env::var_os(SOCKET_ENV), std::env::home_dir())
}

/// The socket the environment names, when it names one: an empty value names nothing. A window
/// that finds one leaves the engine alone.
pub(crate) fn socket_override(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|path| !path.is_empty()).map(PathBuf::from)
}

/// The folder of a profile's data directory that holds the logs. The engine's launch agent writes
/// `engine.log` there, and the window starts the engine with its output going there too.
pub(crate) fn logs_directory(data_directory: &Path) -> PathBuf {
    data_directory.join(LOGS_DIRECTORY)
}

const LOGS_DIRECTORY: &str = "logs";
/// The client's own log file inside [`logs_directory`].
const LOG_FILE: &str = "desktop.log";

/// The file the client's log lines are also written to, so that a program started from a menu or
/// an icon (no terminal; on Windows no console) still leaves them somewhere. It lives in the
/// profile directory under the user's home, on every platform.
pub fn log_file() -> Result<PathBuf> {
    log_file_in(std::env::home_dir())
}

fn log_file_in(home: Option<PathBuf>) -> Result<PathBuf> {
    Ok(logs_directory(&dev_data_directory(home)?).join(LOG_FILE))
}

fn resolve(override_value: Option<OsString>, home: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = socket_override(override_value) {
        return Ok(path);
    }
    Ok(dev_data_directory(home)?.join(DEV_ENGINE_SOCKET))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use soyeht_protocol::profile::DEV_DATA_DIRECTORY_FROM_HOME;

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
            resolve(Some(OsString::new()), Some("/home/a".into())).unwrap(),
            expected
        );
    }

    #[test]
    fn the_log_file_sits_beside_the_engine_log_in_the_profile_directory() {
        let expected = PathBuf::from("/home/a")
            .join(DEV_DATA_DIRECTORY_FROM_HOME)
            .join("logs")
            .join("desktop.log");
        assert_eq!(log_file_in(Some("/home/a".into())).unwrap(), expected);
        assert!(matches!(
            log_file_in(None),
            Err(Error::Protocol(soyeht_protocol::Error::HomeMissing))
        ));
    }

    #[test]
    fn without_a_home_directory_there_is_no_default() {
        assert!(matches!(
            resolve(None, None),
            Err(Error::Protocol(soyeht_protocol::Error::HomeMissing))
        ));
    }
}
