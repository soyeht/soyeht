use soyeht_protocol::HarnessEnvironmentVariable;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

mod cache;
#[cfg(unix)]
mod probe;

static LAUNCH_PATH: cache::Cache = cache::Cache::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchPath {
    pub value: String,
    pub issue: Option<LaunchPathIssue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchPathIssue {
    MissingShell,
    UnsupportedShell,
    UnavailableShell,
    ProbeFailed,
    ProbeTimedOut,
    OutputTooLarge,
    InvalidOutput,
}

impl soyeht_platform::log::Failure for LaunchPathIssue {
    fn failure_kind(&self) -> String {
        match self {
            Self::MissingShell => "missing_shell",
            Self::UnsupportedShell => "unsupported_shell",
            Self::UnavailableShell => "unavailable_shell",
            Self::ProbeFailed => "probe_failed",
            Self::ProbeTimedOut => "probe_timed_out",
            Self::OutputTooLarge => "output_too_large",
            Self::InvalidOutput => "invalid_output",
        }
        .into()
    }
}

/// An interactive login shell knows the `PATH` the person's terminals have; the engine, which
/// the desktop may have started without it, asks that shell and falls back to its own.
#[cfg(unix)]
fn resolve_launch_path() -> LaunchPath {
    match probe::login_shell_path() {
        Ok(value) => LaunchPath { value, issue: None },
        Err(issue) => LaunchPath {
            value: std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            issue: Some(issue),
        },
    }
}

/// A Windows process starts with the `PATH` the person's account has, so there is nothing to
/// ask a shell for.
#[cfg(windows)]
fn resolve_launch_path() -> LaunchPath {
    LaunchPath {
        value: std::env::var("PATH").unwrap_or_default(),
        issue: None,
    }
}

/// Starts the engine with the `PATH` remembered in `directory` and asks the shell again in the
/// background, so a restart does not wait for the shell.
pub fn warm_launch_path(directory: &Path) {
    LAUNCH_PATH.warm(directory, resolve_launch_path);
}

pub fn launch_path_resolution() -> LaunchPath {
    LAUNCH_PATH.resolution(resolve_launch_path)
}

pub fn launch_path() -> String {
    launch_path_resolution().value
}

/// Asks the shell again, for a rescan. Waits for the shell, so it never runs under the Core
/// lock; every other reader keeps the previous answer meanwhile.
pub fn refresh_launch_path() -> LaunchPath {
    LAUNCH_PATH.refresh(resolve_launch_path)
}

pub fn launch_environment() -> BTreeMap<String, String> {
    BTreeMap::from([("PATH".into(), launch_path())])
}

pub fn launch_environment_with(
    recipe: Option<&[HarnessEnvironmentVariable]>,
) -> BTreeMap<String, String> {
    let mut environment = launch_environment();
    for variable in recipe.into_iter().flatten() {
        if !matches!(variable.key.as_str(), "PATH" | "HOME" | "USER" | "LOGNAME")
            && !variable.key.starts_with("SOYEHT_")
        {
            environment.insert(variable.key.clone(), variable.value.clone());
        }
    }
    environment
}

pub fn candidate_directories(recipe_directories: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut seen = BTreeSet::new();
    for directory in std::env::split_paths(&launch_path()) {
        if seen.insert(directory.clone()) {
            directories.push(directory);
        }
    }
    for directory in recipe_directories {
        if seen.insert(directory.clone()) {
            directories.push(directory.clone());
        }
    }
    directories
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_environment_carries_the_resolved_path_to_the_child() {
        let environment = launch_environment();
        assert_eq!(environment.get("PATH"), Some(&launch_path()));
        assert!(!environment.contains_key("HOME"));
    }

    #[test]
    fn recipe_environment_extends_but_cannot_replace_user_identity() {
        let environment = launch_environment_with(Some(&[
            HarnessEnvironmentVariable {
                key: "AGENT_MODE".into(),
                value: "acp".into(),
            },
            HarnessEnvironmentVariable {
                key: "HOME".into(),
                value: "/tmp/empty".into(),
            },
        ]));
        assert_eq!(environment.get("AGENT_MODE"), Some(&"acp".into()));
        assert_eq!(environment.get("PATH"), Some(&launch_path()));
        assert!(!environment.contains_key("HOME"));
    }

    #[test]
    fn recipe_directories_follow_the_shared_fallbacks() {
        let recipe = PathBuf::from("/tmp/recipe-bin");
        let directories = candidate_directories(std::slice::from_ref(&recipe));
        assert_eq!(directories.last(), Some(&recipe));
        let inherited: Vec<_> = std::env::split_paths(&launch_path()).collect();
        assert!(
            directories
                .iter()
                .all(|path| path == &recipe || inherited.contains(path)),
            "no fallback directory is invented beyond the launch path and the recipe"
        );
    }
}
