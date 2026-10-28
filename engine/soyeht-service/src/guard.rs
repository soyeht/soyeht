//! The legacy app is never touched (ADR 0112).
//!
//! The owner's older app is not this repository. Its bundle identifiers, programs and data
//! folders are listed once, in `contracts/legacy.json` (`soyeht_protocol::legacy`); this module
//! only applies the list, and `scripts/legacy_guard.py` applies the same list to scripts.
//!
//! Two kinds of value are checked differently:
//!
//! - **Names and data places** of the app (a label, a unit, a task, the data directory) carry
//!   "next", so no value of the rewrite can equal one of the legacy app's.
//! - **Where the programs are installed** is the installer's choice (`/usr/local/bin`,
//!   `/opt/soyeht`, `C:\Program Files\Soyeht`) and needs no marker. It is refused only when it
//!   is a place of the legacy app.

use crate::error::{Error, Result};
use soyeht_protocol::legacy;
use std::path::Path;

const MARKER: &str = "next";

fn refuse(what: &str, value: &str, reason: &str) -> Error {
    Error::Legacy(format!("{what} \"{value}\" {reason}"))
}

/// Accepts a service name: it carries "next" and is not a legacy bundle identifier. The Mac's
/// file system ignores case, so the comparison does too.
pub fn require_name(what: &str, name: &str) -> Result<()> {
    let lower = name.to_lowercase();
    for id in legacy::BUNDLE_IDS {
        let id = id.to_lowercase();
        let named = lower.find(&id).is_some_and(|start| {
            !lower[start + id.len()..]
                .starts_with(|next: char| next.is_ascii_alphanumeric() || next == '-')
        });
        if named {
            return Err(refuse(
                what,
                name,
                &format!("is or starts with {id}, a bundle identifier of the legacy app"),
            ));
        }
    }
    if !lower.contains(MARKER) {
        return Err(refuse(what, name, &format!("must contain \"{MARKER}\"")));
    }
    Ok(())
}

/// Accepts a place of the app's own data: absolute and plain, it carries "next" and is not a
/// place of the legacy app.
pub fn require_path(what: &str, path: &Path) -> Result<()> {
    require_plain(what, path)?;
    refuse_legacy_place(what, path)?;
    let text = path.to_string_lossy();
    if !text.to_lowercase().contains(MARKER) {
        return Err(refuse(what, &text, &format!("must contain \"{MARKER}\"")));
    }
    Ok(())
}

/// Accepts a place where programs are installed: anywhere an installer puts programs, absolute
/// and plain, and not a place of the legacy app. It needs no "next".
pub fn require_programs(what: &str, path: &Path) -> Result<()> {
    require_plain(what, path)?;
    refuse_legacy_place(what, path)
}

/// A drive path (`C:\...`) or a network path (`\\...`), by its text, so a definition for
/// Windows is judged the same on any host.
fn is_windows_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    let drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive || text.starts_with("\\\\")
}

/// Refuses a path that is not absolute or that has a `.` or `..` component. The checks below
/// read the names in the path, so a `..` that leads out of a folder named "next" into a legacy
/// one would pass them; and a definition holds absolute paths, since the system starts it from
/// somewhere else.
fn require_plain(what: &str, path: &Path) -> Result<()> {
    let text = path.to_string_lossy();
    if !path.is_absolute() && !is_windows_absolute(&text) {
        return Err(refuse(what, &text, "must be an absolute path"));
    }
    if text
        .split(['/', '\\'])
        .any(|part| part == ".." || part == ".")
    {
        return Err(refuse(what, &text, "must not contain a . or .. component"));
    }
    Ok(())
}

/// Refuses a path inside one of the legacy programs or one of its data folders. A folder is
/// matched as a whole component, case-insensitively, so a name that only starts with a legacy
/// one (a `.next` folder beside it) is the rewrite's.
fn refuse_legacy_place(what: &str, path: &Path) -> Result<()> {
    let text = path.to_string_lossy();
    let parts: Vec<String> = text
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect();
    for application in legacy::APPLICATIONS {
        if parts.contains(&application.to_lowercase()) {
            return Err(refuse(
                what,
                &text,
                &format!("is inside {application}, the legacy program"),
            ));
        }
    }
    let base = legacy::DATA_BASE.to_lowercase();
    for folder in legacy::DATA_FOLDERS {
        let folder_lower = folder.to_lowercase();
        if parts
            .windows(2)
            .any(|pair| pair[0] == base && pair[1] == folder_lower)
        {
            return Err(refuse(
                what,
                &text,
                &format!(
                    "is inside {}/{folder}, a data folder of the legacy app",
                    legacy::DATA_BASE
                ),
            ));
        }
    }
    Ok(())
}

/// The legacy app's names and places, for the tests of the crates that must refuse them.
#[cfg(test)]
pub(crate) mod legacy_values {
    use soyeht_protocol::legacy;
    use std::path::{Path, PathBuf};

    /// A service name of the legacy app's family.
    pub(crate) fn name() -> String {
        format!("{}.engine", legacy::BUNDLE_IDS[0])
    }

    /// The legacy app's data folder below `base`.
    pub(crate) fn data(base: &Path) -> PathBuf {
        base.join("Library")
            .join(legacy::DATA_BASE)
            .join(legacy::DATA_FOLDERS[0])
    }

    /// The legacy app's program.
    pub(crate) fn program() -> PathBuf {
        PathBuf::from(legacy::APPLICATION_FOLDER)
            .join(legacy::APPLICATIONS[0])
            .join("Contents")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::profile::PROFILES;
    use std::path::PathBuf;

    #[test]
    fn the_names_of_every_profile_pass() {
        for profile in PROFILES {
            for name in [
                profile.engine_launchd_label,
                profile.ptyd_launchd_label,
                profile.engine_systemd_unit,
                profile.ptyd_windows_task,
            ] {
                require_name("name", name).unwrap();
            }
        }
    }

    #[test]
    fn a_legacy_or_unmarked_name_is_refused_in_any_case_and_the_error_says_why() {
        for id in legacy::BUNDLE_IDS {
            for name in [
                id.to_owned(),
                id.to_uppercase(),
                format!("{id}.engine"),
                format!("{id}.next"),
            ] {
                let error = require_name("name", &name).unwrap_err().to_string();
                assert!(
                    error.contains("bundle identifier of the legacy app"),
                    "{name}: {error}"
                );
            }
        }
        for name in ["com.soyeht.engine", "soyeht-engine.service", ""] {
            let error = require_name("name", name).unwrap_err().to_string();
            assert!(error.contains("must contain \"next\""), "{name}: {error}");
        }
        // A name that only starts like the legacy one, but is another word, is the next app's.
        require_name("name", &format!("{}hine.next", legacy::BUNDLE_IDS[0])).unwrap();
    }

    #[test]
    fn a_programs_folder_needs_no_marker() {
        for path in [
            "/usr/bin",
            "/usr/local/bin",
            "/opt/soyeht",
            "/home/a/.local/bin",
            "C:\\Program Files\\Soyeht",
            "/Applications/Soyeht.next.app/Contents/MacOS",
            "/Users/a/Library/Application Support/Next/bin",
        ] {
            require_programs("path", &PathBuf::from(path)).unwrap();
        }
    }

    #[test]
    fn a_programs_folder_is_never_a_place_of_the_legacy_app() {
        for (path, said) in [
            ("/Applications/Soyeht.app", "inside Soyeht.app"),
            (
                "/APPLICATIONS/soyeht.app/Contents/MacOS",
                "inside Soyeht.app",
            ),
            (
                "/Applications/Soyeht Dev.app/Contents",
                "inside Soyeht Dev.app",
            ),
            (
                "/Users/a/Applications/Soyeht Dev.app",
                "inside Soyeht Dev.app",
            ),
            ("C:\\Apps\\Soyeht.app\\bin", "inside Soyeht.app"),
            (
                "/Users/a/Library/Application Support/Soyeht/bin",
                "Application Support/Soyeht,",
            ),
            (
                "/Users/a/Library/Application Support/SOYEHT DEV",
                "Application Support/Soyeht Dev,",
            ),
        ] {
            let error = require_programs("path", &PathBuf::from(path)).unwrap_err();
            assert!(error.to_string().contains(said), "{path}: {error}");
        }
        assert!(require_programs("path", &legacy_values::program()).is_err());
    }

    #[test]
    fn a_path_must_be_absolute_and_free_of_dot_components() {
        let hidden = "/Users/a/Library/Application Support/next/../Soyeht";
        for path in [
            hidden,
            "/Users/a/Library/Application Support/next/../../Soyeht",
            "/opt/next/./bin",
            "/opt/next/bin/..",
            "relative/next",
            "next",
            "",
        ] {
            let path = PathBuf::from(path);
            assert!(require_path("path", &path).is_err(), "{path:?}");
            assert!(require_programs("path", &path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn a_data_place_needs_the_marker_and_is_never_a_legacy_place() {
        for path in [
            "/Users/a/Library/Application Support/Soyeht/next",
            "/Users/a/Library/Application Support/Plain",
            "/opt/soyeht",
        ] {
            assert!(
                require_path("path", &PathBuf::from(path)).is_err(),
                "{path}"
            );
        }
        for path in [
            "/Users/a/Library/Application Support/Next/dev",
            "/Users/a/.local/share/next",
            "C:\\Users\\a\\AppData\\Local\\Next",
        ] {
            require_path("path", &PathBuf::from(path)).unwrap();
        }
    }
}
