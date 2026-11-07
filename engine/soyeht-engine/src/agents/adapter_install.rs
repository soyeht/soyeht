//! Built-in adapters installed once from a lockfile, and run from there (ADR 0143, amended).
//!
//! `npx --yes <package>@<version>` pins the adapter's own version and nothing else: every package it
//! depends on is resolved from the registry at that moment, and its install scripts run. The
//! common adapters are instead installed once, with `npm ci` from a lockfile the engine carries,
//! which holds the exact version and the integrity hash of every package of the tree, and
//! with scripts off. A package whose bytes differ from the lock is refused by npm. A launch of the
//! pinned `npx` command is rewritten to `node <the installed entry>` where the start is sent, off
//! the Core lock, so a pane, the candidate of a harness switch and a sign-in terminal all run the
//! same installed tree.
//!
//! Anything else keeps its package runner. The lockfiles are `adapter_locks/<name>/`; a test ties
//! each to the package and version the registry snapshot pins, so a snapshot that moves on without
//! its lock fails the gate and does not silently go back to `npx`.

use super::{
    adapter_directory,
    package_runner::{PackageRunner, package_runner},
};
use crate::{
    errors::{self, Error, Result},
    streak::{Streak, first_failure, recovered},
};
use sha2::{Digest, Sha256};
use soyeht_platform::{fs::ensure_private_dir, log};
use soyeht_process::Limits;
use soyeht_ptyd::wire::Spawn;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::Duration,
};

/// The folder of the installed adapters, inside the adapters folder.
const INSTALLED: &str = "installed";
/// The file of an installed tree that names the lock it was installed from.
const MARKER: &str = ".soyeht-lock";
/// `npm ci` fetches the whole tree.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const INSTALL_OUTPUT_BYTES: usize = 64 * 1024;

/// An adapter the engine installs from a lockfile.
pub(crate) struct Locked {
    /// The registry package and its pinned version, as the snapshot's launch names them.
    pub(crate) package: &'static str,
    pub(crate) version: &'static str,
    /// The folder of the lockfiles, and of the installed tree.
    name: &'static str,
    /// The adapter's entry script, below `node_modules`.
    entry: &'static str,
    manifest: &'static str,
    lock: &'static str,
}

pub(crate) const LOCKED: [Locked; 2] = [
    Locked {
        package: "@agentclientprotocol/claude-agent-acp",
        version: "0.100.0",
        name: "claude-agent-acp",
        entry: "@agentclientprotocol/claude-agent-acp/dist/index.js",
        manifest: include_str!("adapter_locks/claude-agent-acp/package.json"),
        lock: include_str!("adapter_locks/claude-agent-acp/package-lock.json"),
    },
    Locked {
        package: "@agentclientprotocol/codex-acp",
        version: "1.11.0",
        name: "codex-acp",
        entry: "@agentclientprotocol/codex-acp/dist/index.js",
        manifest: include_str!("adapter_locks/codex-acp/package.json"),
        lock: include_str!("adapter_locks/codex-acp/package-lock.json"),
    },
];

/// The failure to install, which repeats with each start.
static FAILURES: Mutex<Streak> = Mutex::new(Streak::once());
/// One install at a time: a pane and a harness switch can start the same adapter together.
static INSTALLING: Mutex<()> = Mutex::new(());

impl Locked {
    /// The digest of the lock, which an installed tree carries to show what it was made from.
    fn digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.manifest.as_bytes());
        hash.update([0]);
        hash.update(self.lock.as_bytes());
        format!("{:x}", hash.finalize())
    }

    fn folder(&self, profile: &Path) -> PathBuf {
        adapter_directory::path(profile)
            .join(INSTALLED)
            .join(format!("{}-{}", self.name, self.version))
    }

    fn entry_path(&self, folder: &Path) -> PathBuf {
        folder.join("node_modules").join(self.entry)
    }

    /// Whether `folder` holds a finished install of this lock.
    fn installed(&self, folder: &Path) -> bool {
        fs::read_to_string(folder.join(MARKER)).is_ok_and(|marker| marker == self.digest())
            && self.entry_path(folder).is_file()
    }
}

/// The locked adapter `argv` launches, and the arguments after the package: exactly
/// `npx --yes <package>@<version> ...`.
fn locked_launch(argv: &[String]) -> Option<(&'static Locked, &[String])> {
    let [runner, yes, package, rest @ ..] = argv else {
        return None;
    };
    (package_runner(runner) == Some(PackageRunner::Npx) && yes == "--yes").then_some(())?;
    LOCKED
        .iter()
        .find(|locked| *package == format!("{}@{}", locked.package, locked.version))
        .map(|locked| (locked, rest))
}

/// The package a built-in adapter downloads and runs, as `name@version` (`name==version` for
/// `uvx`), and whether the engine installs it from its lock. `None` for any launch that is not a
/// built-in adapter of a package runner. What a picker tells the person before the first start.
pub(crate) fn runner_package(launch: &soyeht_protocol::HarnessLaunch) -> Option<(String, bool)> {
    let runner = super::package_runner::builtin_package_runner(launch)?;
    let package = match runner {
        PackageRunner::Npx => launch.args.get(1),
        PackageRunner::Uvx => launch.args.first(),
    }?;
    let argv: Vec<String> = std::iter::once(launch.command.clone())
        .chain(launch.args.iter().cloned())
        .collect();
    Some((package.clone(), locked_launch(&argv).is_some()))
}

fn npm_program() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

/// Installs `locked` into `folder` if it is not there: `npm ci` with scripts off, in a staging
/// folder that holds only the engine's own manifest and lock, then moved into place. Off the
/// Core lock.
fn install(locked: &Locked, profile: &Path, folder: &Path, npm: &str) -> Result<()> {
    let root = folder
        .parent()
        .ok_or_else(|| errors::internal("Adapter folder has no parent"))?;
    ensure_private_dir(&adapter_directory::path(profile)).map_err(Error::from)?;
    ensure_private_dir(root).map_err(Error::from)?;
    let staging = root.join(format!(".{}-{}", locked.name, uuid::Uuid::new_v4()));
    ensure_private_dir(&staging).map_err(Error::from)?;
    let result = stage(locked, &staging, npm).and_then(|()| {
        // A tree that is there but is not this lock (another version of the lock, a half install)
        // is replaced.
        if folder.exists() {
            fs::remove_dir_all(folder)?;
        }
        fs::rename(&staging, folder)?;
        Ok(())
    });
    if result.is_err() {
        // The staging folder is the engine's own; a failure to remove it is retried by the next
        // install, which makes a new one.
        drop(fs::remove_dir_all(&staging));
    }
    result
}

fn stage(locked: &Locked, staging: &Path, npm: &str) -> Result<()> {
    fs::write(staging.join("package.json"), locked.manifest)?;
    fs::write(staging.join("package-lock.json"), locked.lock)?;
    let mut command = Command::new(npm);
    command
        .args([
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ])
        .current_dir(staging)
        .env("PATH", super::environment::launch_search_path());
    let output = soyeht_process::run(command, Limits::new(INSTALL_TIMEOUT, INSTALL_OUTPUT_BYTES))
        .map_err(|error| errors::harness_unavailable(error.to_string()))?;
    if !output.success() {
        return Err(errors::harness_unavailable(
            "The adapter's packages could not be installed from the engine's lock",
        ));
    }
    // npm checked every package against the lock. The tree must also be the adapter asked for.
    let manifest = staging
        .join("node_modules")
        .join(locked.package)
        .join("package.json");
    let installed: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest)?)
        .map_err(|_| {
            errors::harness_unavailable("The installed adapter has no readable manifest")
        })?;
    let version = installed["version"].as_str();
    if version != Some(locked.version) || !locked.entry_path(staging).is_file() {
        return Err(errors::harness_unavailable(
            "The installed adapter is not the version the engine pins",
        ));
    }
    fs::write(staging.join(MARKER), locked.digest())?;
    Ok(())
}

/// The installed entry of `locked`, installed first when it is not there.
fn ensure(locked: &Locked, profile: &Path, npm: &str) -> Result<PathBuf> {
    let folder = locked.folder(profile);
    if locked.installed(&folder) {
        return Ok(locked.entry_path(&folder));
    }
    let _one_at_a_time = INSTALLING
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if !locked.installed(&folder) {
        match install(locked, profile, &folder, npm) {
            Ok(()) => recovered(&FAILURES),
            Err(error) => {
                if first_failure(&FAILURES, &log::Failure::failure_kind(&error)) {
                    log::failure("engine.adapter_install_failed", &error);
                }
                return Err(error);
            }
        }
    }
    Ok(locked.entry_path(&folder))
}

/// `config` with the launch of a locked adapter replaced by the installed one: `node <entry>` and
/// the arguments after the package. Any other process is returned as it is. It runs `npm` the
/// first time, so it runs where the start is sent, off the Core lock.
pub(crate) fn installed_spawn(profile: &Path, config: Spawn) -> Result<Spawn> {
    installed_spawn_with(profile, config, npm_program())
}

fn installed_spawn_with(profile: &Path, mut config: Spawn, npm: &str) -> Result<Spawn> {
    let Some((locked, rest)) = locked_launch(&config.argv) else {
        return Ok(config);
    };
    let entry = ensure(locked, profile, npm)?;
    let entry = entry
        .to_str()
        .ok_or_else(|| errors::internal("Adapter path is not UTF-8"))?
        .to_owned();
    let mut argv = vec!["node".to_owned(), entry];
    argv.extend(rest.iter().cloned());
    config.argv = argv;
    Ok(config)
}

#[cfg(test)]
mod tests;
