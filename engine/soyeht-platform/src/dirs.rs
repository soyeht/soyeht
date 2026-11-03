//! The folder where a user's applications keep their data on this system.
//!
//! It is the one place that knows the native base directory: `~/Library/Application Support`
//! on macOS, `%LOCALAPPDATA%` on Windows, and `$XDG_DATA_HOME` (else `~/.local/share`) on
//! Linux. The profile (`contracts/profiles.json`) names the folder inside it; this crate does
//! not know the profile.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum System {
    MacOs,
    Windows,
    Linux,
}

impl System {
    const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// The base directory for application data of the user running this program, or `None` when
/// the system does not say where the user's home is.
pub fn data_base() -> Option<PathBuf> {
    base(
        System::current(),
        std::env::home_dir(),
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("XDG_DATA_HOME"),
    )
}

fn base(
    system: System,
    home: Option<PathBuf>,
    local_app_data: Option<OsString>,
    xdg_data_home: Option<OsString>,
) -> Option<PathBuf> {
    match system {
        System::MacOs => home.map(|home| home.join("Library").join("Application Support")),
        System::Windows => {
            absolute(local_app_data).or_else(|| home.map(|home| home.join("AppData").join("Local")))
        }
        // The XDG specification ignores a relative `XDG_DATA_HOME`.
        System::Linux => {
            absolute(xdg_data_home).or_else(|| home.map(|home| home.join(".local").join("share")))
        }
    }
}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value
        .map(PathBuf::from)
        .filter(|path| Path::new(path).is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> Option<PathBuf> {
        Some(PathBuf::from("/home/a"))
    }

    #[test]
    fn macos_keeps_data_in_application_support_under_home() {
        assert_eq!(
            base(System::MacOs, home(), Some("/ignored".into()), None),
            Some(PathBuf::from("/home/a/Library/Application Support"))
        );
        assert_eq!(base(System::MacOs, None, None, None), None);
    }

    // Written with Unix paths: on Windows "/data" has no drive and is not absolute.
    #[cfg(unix)]
    #[test]
    fn linux_follows_xdg_data_home_and_ignores_a_relative_one() {
        assert_eq!(
            base(System::Linux, home(), None, Some("/data".into())),
            Some(PathBuf::from("/data"))
        );
        for unusable in [OsString::new(), "relative/dir".into()] {
            assert_eq!(
                base(System::Linux, home(), None, Some(unusable)),
                Some(PathBuf::from("/home/a/.local/share"))
            );
        }
        assert_eq!(base(System::Linux, None, None, None), None);
    }

    #[test]
    fn windows_uses_local_app_data_else_the_profile_folder() {
        let local = if cfg!(windows) {
            r"C:\Users\a\AppData\Local"
        } else {
            "/users/a/local"
        };
        assert_eq!(
            base(System::Windows, home(), Some(local.into()), None),
            Some(PathBuf::from(local))
        );
        assert_eq!(
            base(System::Windows, home(), None, None),
            Some(PathBuf::from("/home/a/AppData/Local"))
        );
    }
}
