//! The folder a package-runner adapter starts in.
//!
//! `npx` and `uvx` read the configuration and the packages of the folder they start in (an
//! `.npmrc`, a `node_modules`, a `.python-version`). A built-in adapter is launched through one,
//! so it must not start in the workspace, where a repository could decide which code runs. It
//! starts in a folder the engine owns and empty. The session's folder reaches the adapter through
//! ACP (`session/new` and `session/load` carry `cwd`), which is where the adapter works.

use soyeht_platform::{fs::ensure_private_dir, log};
use std::path::{Path, PathBuf};

/// The name of the folder, inside the engine's data directory.
const DIRECTORY: &str = "adapters";

/// The runners a registry adapter may be launched through, by file stem (`npx.cmd` is `npx`).
const RUNNERS: &[&str] = &["npx", "npx-cli", "uvx", "uv"];

/// The folder package-runner adapters start in.
pub(crate) fn path(profile: &Path) -> PathBuf {
    profile.join(DIRECTORY)
}

/// Makes the folder, owner-only. Without it the adapters do not start, which is better than
/// starting them in a workspace: the failure is logged and the engine goes on.
pub(crate) fn prepare(profile: &Path) {
    if let Err(error) = ensure_private_dir(&path(profile)) {
        log::failure(
            "engine.adapter_directory_failed",
            &crate::errors::Error::from(error),
        );
    }
}

/// Whether `command` is a package runner.
pub(crate) fn is_package_runner(command: &str) -> bool {
    Path::new(command)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| {
            RUNNERS
                .iter()
                .any(|runner| stem.eq_ignore_ascii_case(runner))
        })
}

/// The folder the process of `command` starts in: the engine's own for a package runner, the
/// pane's `cwd` for anything else.
pub(crate) fn start_in(profile: &Path, command: &str, cwd: &str) -> String {
    if is_package_runner(command) {
        path(profile).display().to_string()
    } else {
        cwd.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_runners_are_known_by_their_file_stem() {
        for command in [
            "npx",
            "/opt/homebrew/bin/npx",
            "C:/Program Files/nodejs/npx.cmd",
            "/node/bin/npx-cli.js",
            "uvx",
            "/usr/local/bin/uvx",
            "uv.exe",
        ] {
            assert!(is_package_runner(command), "{command}");
        }
        for command in [
            "codex",
            "/usr/local/bin/claude",
            "node",
            "/bin/sh",
            "npxe",
            "",
        ] {
            assert!(!is_package_runner(command), "{command}");
        }
    }

    #[test]
    fn a_runner_starts_in_the_engines_folder_and_a_program_in_the_panes() {
        let profile = Path::new("/profile");
        assert_eq!(
            start_in(profile, "/bin/npx", "/work/repository"),
            profile.join(DIRECTORY).display().to_string()
        );
        assert_eq!(
            start_in(profile, "/bin/gemini", "/work/repository"),
            "/work/repository"
        );
    }

    #[test]
    fn the_folder_is_made_owner_only_and_is_not_the_workspace() {
        let profile = soyeht_platform::testing::scratch_dir("adapter-directory");
        prepare(&profile);
        assert!(soyeht_platform::fs::dir_is_owner_only(&path(&profile)).unwrap());
        assert!(std::fs::read_dir(path(&profile)).unwrap().next().is_none());
        prepare(&profile);
    }
}
