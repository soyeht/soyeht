use soyeht_protocol::HarnessEnvironmentVariable;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{OnceLock, RwLock},
};

#[cfg(unix)]
mod probe;

static LAUNCH_PATH: OnceLock<RwLock<Option<LaunchPath>>> = OnceLock::new();

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

pub fn launch_path_resolution() -> LaunchPath {
    let cache = LAUNCH_PATH.get_or_init(|| RwLock::new(None));
    // A poisoned cache only costs a fresh probe.
    if let Some(path) = cache.read().ok().and_then(|value| value.clone()) {
        return path;
    }
    let path = resolve_launch_path();
    if let Ok(mut value) = cache.write() {
        *value = Some(path.clone());
    }
    path
}

pub fn launch_path() -> String {
    launch_path_resolution().value
}

pub fn invalidate_launch_path() {
    if let Some(cache) = LAUNCH_PATH.get()
        && let Ok(mut value) = cache.write()
    {
        *value = None;
    }
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
