//! The folder a built-in package-runner adapter starts in.
//!
//! `npx` and `uvx` read the configuration and the packages of the folder they start in (an
//! `.npmrc`, a `node_modules`, a `.python-version`). A built-in registry adapter is launched
//! through one, so it must not start in the workspace, where a repository could decide which code
//! runs. It starts in a folder the engine owns, owner-only, and empty but for `installed/`, where
//! the engine puts the adapters it installs from a lockfile (`adapter_install`). The session's folder
//! reaches the adapter through ACP (`session/new` and `session/load` carry `cwd`), which is where
//! the adapter works (ADR 0143).
//!
//! Only the built-in entries move: a custom harness runs as its author wrote it, with its own
//! relative paths, in the pane's folder.

use crate::streak::{Streak, first_failure, recovered};
use soyeht_platform::{fs::ensure_private_dir, log};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// The name of the folder, inside the engine's data directory.
const DIRECTORY: &str = "adapters";

/// The failure to make the folder, which repeats with each start.
static FAILURES: Mutex<Streak> = Mutex::new(Streak::once());

/// The folder package-runner adapters start in.
pub(crate) fn path(profile: &Path) -> PathBuf {
    profile.join(DIRECTORY)
}

/// Makes `directory`, the folder [`path`] names, owner-only when it is not there (or not
/// private). It touches the disk (a stat, and a mkdir and a sync when the folder is missing), so
/// it runs where the start of the process is sent, off the Core lock ([`Launch::send`]), before
/// each start: a folder that was removed is made again. Without it the adapter does not start,
/// which is better than starting it in a workspace: the failure is logged by streak.
///
/// [`Launch::send`]: crate::process_start::Launch::send
pub(crate) fn prepare(directory: &Path) {
    match ensure_private_dir(directory) {
        Ok(()) => recovered(&FAILURES),
        Err(error) => {
            let error = crate::errors::Error::from(error);
            if first_failure(&FAILURES, &log::Failure::failure_kind(&error)) {
                log::failure("engine.adapter_directory_failed", &error);
            }
        }
    }
}

/// The folder to [`prepare`] before a process that starts in `cwd` is sent: the engine's own, when
/// that is where it starts. Computed under the lock (it touches nothing), used off it.
pub(crate) fn to_prepare(profile: &Path, cwd: &str) -> Option<PathBuf> {
    let directory = path(profile);
    (Path::new(cwd) == directory).then_some(directory)
}

/// The folder the process of a launch starts in: the engine's own when `in_engine_folder` (the
/// caller read it from `package_runner::starts_in_engine_folder`), the pane's `cwd` for anything
/// else. It only names the folder; [`prepare`] makes it.
pub(crate) fn start_in(profile: &Path, in_engine_folder: bool, cwd: &str) -> String {
    if in_engine_folder {
        path(profile).display().to_string()
    } else {
        cwd.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_starts_in_the_engines_folder_or_the_panes() {
        let profile = Path::new("/profile");
        assert_eq!(
            start_in(profile, true, "/work/repository"),
            path(profile).display().to_string()
        );
        assert_eq!(
            start_in(profile, false, "/work/repository"),
            "/work/repository"
        );
        // Naming the folder touches nothing.
        assert!(!path(profile).exists());
    }

    #[test]
    fn the_folder_is_made_owner_only_and_again_when_it_is_removed() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-directory");
        let directory = path(&profile);
        prepare(&directory);
        assert!(soyeht_platform::fs::dir_is_owner_only(&directory).unwrap());
        assert!(std::fs::read_dir(&directory).unwrap().next().is_none());
        std::fs::remove_dir(&directory).unwrap();
        prepare(&directory);
        assert!(soyeht_platform::fs::dir_is_owner_only(&directory).unwrap());
    }
}
