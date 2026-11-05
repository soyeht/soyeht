//! The folder a built-in package-runner adapter starts in.
//!
//! `npx` and `uvx` read the configuration and the packages of the folder they start in (an
//! `.npmrc`, a `node_modules`, a `.python-version`). A built-in registry adapter is launched
//! through one, so it must not start in the workspace, where a repository could decide which code
//! runs. It starts in a folder the engine owns, owner-only, and empty. The session's folder
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

/// Makes the folder, owner-only, when it is not there (or not private). Called before each start,
/// so a folder that was removed is made again. Without it the adapter does not start, which is
/// better than starting it in a workspace: the failure is logged by streak.
fn prepare(profile: &Path) {
    match ensure_private_dir(&path(profile)) {
        Ok(()) => recovered(&FAILURES),
        Err(error) => {
            let error = crate::errors::Error::from(error);
            if first_failure(&FAILURES, &log::Failure::failure_kind(&error)) {
                log::failure("engine.adapter_directory_failed", &error);
            }
        }
    }
}

/// The folder the process of a launch starts in: the engine's own for a built-in package-runner
/// adapter (`package_runner`, which the caller read from `registry::starts_in_engine_folder`), the pane's `cwd`
/// for anything else.
pub(crate) fn start_in(profile: &Path, package_runner: bool, cwd: &str) -> String {
    if package_runner {
        prepare(profile);
        path(profile).display().to_string()
    } else {
        cwd.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runner_starts_in_the_engines_folder_and_a_program_in_the_panes() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-start");
        assert_eq!(
            start_in(&profile, true, "/work/repository"),
            path(&profile).display().to_string()
        );
        assert_eq!(
            start_in(&profile, false, "/work/repository"),
            "/work/repository"
        );
    }

    #[test]
    fn the_folder_is_made_owner_only_before_a_start_and_again_when_it_is_removed() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-directory");
        start_in(&profile, true, "/work");
        assert!(soyeht_platform::fs::dir_is_owner_only(&path(&profile)).unwrap());
        assert!(std::fs::read_dir(path(&profile)).unwrap().next().is_none());
        std::fs::remove_dir(path(&profile)).unwrap();
        start_in(&profile, true, "/work");
        assert!(soyeht_platform::fs::dir_is_owner_only(&path(&profile)).unwrap());
    }

    #[test]
    fn a_program_that_is_not_a_runner_makes_no_folder() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-none");
        start_in(&profile, false, "/work");
        assert!(!path(&profile).exists());
    }
}
