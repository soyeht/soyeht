//! Starting `git` without opening the macOS installer. See [`soyeht_platform::git`].

use crate::{Error, Limits, Output, Result, run};
use soyeht_platform::git::{Availability, Probe};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// What a person is told when `git` is not started. It names the cause and the way out, and
/// says Soyeht did not open the installer.
pub const GIT_UNAVAILABLE_MESSAGE: &str = "Git is not available on this computer. Soyeht did not start it, because on a Mac without the Command Line Tools that opens the system installer. Install Git or the Command Line Tools when you choose, then try again.";
/// Fails with [`Error::GitUnavailable`] when `git` cannot run on this machine, without
/// starting it. On a Mac without the Command Line Tools, starting `/usr/bin/git` opens the
/// system installer, so every path that starts `git`, or a program that starts it, calls this
/// first (or uses [`run_git`]).
pub fn require_git() -> Result<()> {
    require_git_with(&Probe::system())
}

pub fn require_git_with(probe: &Probe) -> Result<()> {
    match probe.availability() {
        Availability::Available => Ok(()),
        Availability::Unavailable => Err(Error::GitUnavailable),
    }
}

/// [`run`] for a `git` command, after [`require_git`].
pub fn run_git(command: Command, limits: Limits) -> Result<Output> {
    run_git_with(&Probe::system(), command, limits)
}

pub fn run_git_with(probe: &Probe, command: Command, limits: Limits) -> Result<Output> {
    require_git_with(probe)?;
    run(command, limits)
}

/// A hooks folder that holds nothing: git finds no hook there.
const NO_HOOKS: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
/// The settings of a filter driver that run a program when git compares a file with the index:
/// the driver's conversions and its long-running process.
const FILTER_PROGRAMS: [&str; 3] = ["clean", "smudge", "process"];

/// What it takes to run `git` on one repository so that it reads it and starts nothing the
/// repository configures. Prepare it once for a series of commands on the same repository, and
/// take a [`command`](Self::command) from it for each; the caller adds the arguments and runs the
/// command with [`run_git_with`].
///
/// A repository carries its own configuration, and some of it names programs that git starts
/// while it only reads: the file system monitor (`core.fsmonitor`) and every filter driver that
/// `.gitattributes` assigns to a path (`filter.<name>.clean`, `.smudge`, `.process`). A command
/// turns the monitor off and clears the programs of each driver that the repository's own
/// configuration names: the drivers of its `config` and of every file that configuration includes,
/// whatever the condition of the include (an `includeIf` can hold only inside a worktree not made
/// yet, so the files are read whether or not it holds), and the `config.worktree` of the main
/// worktree and of every linked one (`wt` runs git in each). More included
/// files than the cap, an include git would expand differently (`~user/`) and an unreadable file are errors.
/// The drivers of the person's global and
/// system configuration (git-lfs, git-crypt) are theirs and stay. Textconv and external diff
/// programs are not in this list: they are off with `--no-textconv` and `--no-ext-diff` on the
/// commands that have them, which the caller passes. A submodule is a repository with its own
/// configuration, which a command that looks inside it (`diff` and `status` do) reads with its
/// own filters: the caller passes `--ignore-submodules=dirty` so that it does not look.
///
/// It also never pages, takes no optional lock and reads paths literally. A configuration whose
/// driver names cannot be listed or cannot be written as `-c` settings is an error: nothing is
/// prepared.
///
/// Not covered, because the commands this is made for do not run them: `diff.external` and
/// textconv programs (`--no-ext-diff` and `--no-textconv` switch them off where a caller diffs),
/// `gpg.program` and the merge drivers. A caller that starts worktrunk's `merge` or `step`, or any
/// command that diffs, signs or merges, must cover them first.
///
/// The drivers are listed once, when this is prepared. A configuration that changes after that, by
/// another process that writes the repository's config, is not seen by the commands taken from it:
/// the check and the use are not one step, and what is ruled out is the configuration the
/// repository already had.
pub struct ReadOnlyGit {
    repository: PathBuf,
    settings: Vec<String>,
}

impl ReadOnlyGit {
    pub fn prepare_with(probe: &Probe, repository: &Path, limits: Limits) -> Result<Self> {
        let names = repository_drivers(probe, repository, limits)?;
        // No hook runs either: a command that only reads starts none, and one that changes a
        // checkout (a worktree add) must not start the program a repository names for it.
        let mut settings = vec![
            "core.fsmonitor=false".to_owned(),
            format!("core.hooksPath={NO_HOOKS}"),
        ];
        for name in &names {
            for setting in FILTER_PROGRAMS {
                settings.push(format!("filter.{name}.{setting}="));
            }
            settings.push(format!("filter.{name}.required=false"));
        }
        Ok(Self {
            repository: repository.to_owned(),
            settings,
        })
    }

    /// [`prepare_with`](Self::prepare_with) for this machine's `git`.
    pub fn prepare(repository: &Path, limits: Limits) -> Result<Self> {
        Self::prepare_with(&Probe::system(), repository, limits)
    }

    /// A `git` command for the repository, with no arguments yet.
    pub fn command(&self) -> Command {
        let mut command = Command::new("git");
        command.arg("--no-pager").arg("-C").arg(&self.repository);
        for setting in &self.settings {
            command.arg("-c").arg(setting);
        }
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env("GIT_PAGER", "cat");
        command
    }
}

impl ReadOnlyGit {
    /// The same settings for every `git` that another program starts: `command` is a program
    /// that runs git itself (worktrunk), so the settings travel in the environment
    /// (`GIT_CONFIG_COUNT`), which git reads as if they were `-c` options. They come after the
    /// entries the environment already carries, so they win and those stay. Git before 2.31
    /// ignores these variables: [`require_config_environment`] refuses to go on with it.
    ///
    /// Fails, and leaves the command alone, when the environment's own `GIT_CONFIG_COUNT` is not
    /// a number or leaves no room for these settings: git would read the entries differently
    /// from here, so the command is not run hardened.
    pub fn harden_environment(&self, command: &mut Command) -> Result<()> {
        let inherited = match std::env::var("GIT_CONFIG_COUNT") {
            Ok(count) => Some(count),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(refused("GIT_CONFIG_COUNT is not text"));
            }
        };
        self.harden_over(command, inherited.as_deref())
    }

    /// [`harden_environment`](Self::harden_environment) over the given `GIT_CONFIG_COUNT`.
    fn harden_over(&self, command: &mut Command, inherited: Option<&str>) -> Result<()> {
        let inherited = match inherited {
            Some(count) => count
                .parse::<usize>()
                .map_err(|_| refused("GIT_CONFIG_COUNT is not a number"))?,
            None => 0,
        };
        let total = inherited
            .checked_add(self.settings.len())
            .ok_or_else(|| refused("GIT_CONFIG_COUNT leaves no room for the settings"))?;
        command.env("GIT_CONFIG_COUNT", total.to_string());
        for (index, setting) in self.settings.iter().enumerate() {
            // `prepare_with` wrote every setting as `key=value`.
            let (key, value) = setting.split_once('=').unwrap_or((setting, ""));
            // `inherited + index` is below `total`, which did not overflow.
            let number = inherited + index;
            command.env(format!("GIT_CONFIG_KEY_{number}"), key);
            command.env(format!("GIT_CONFIG_VALUE_{number}"), value);
        }
        Ok(())
    }
}

/// The oldest git that reads `GIT_CONFIG_COUNT`.
const CONFIG_ENVIRONMENT_FROM: (u32, u32) = (2, 31);

/// Fails with [`Error::GitTooOld`] when `git` is older than the one that reads settings from the
/// environment, so that [`ReadOnlyGit::harden_environment`] would harden nothing.
pub fn require_config_environment(probe: &Probe, limits: Limits) -> Result<()> {
    let mut version = Command::new("git");
    version.arg("--version");
    let output = run_git_with(probe, version, limits)?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    match git_version(&text) {
        Some(found) if found >= CONFIG_ENVIRONMENT_FROM => Ok(()),
        _ => Err(Error::GitTooOld {
            found: text.trim().chars().take(64).collect(),
        }),
    }
}

/// `(major, minor)` of `git version 2.50.1 (Apple Git-155)`.
fn git_version(text: &str) -> Option<(u32, u32)> {
    let numbers = text
        .trim()
        .strip_prefix("git version ")?
        .split_whitespace()
        .next()?;
    let mut parts = numbers.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// The filter drivers the repository's own configuration names.
///
/// The configuration is read with `--no-includes`, so that each entry shows the file it is in and
/// the `include.path` and `includeIf.<condition>.path` entries show where they point. Entries of
/// the repository's scopes (`local`, `worktree`) count, and so does everything in a file those
/// point to, at any depth: the condition of an `includeIf` is not evaluated, because it can be
/// true only in a worktree that is made later (`gitdir:` of a worktree, `onbranch:`).
fn repository_drivers(
    probe: &Probe,
    repository: &Path,
    limits: Limits,
) -> Result<BTreeSet<String>> {
    let read = |file: Option<&Path>| -> Result<Vec<ConfigEntry>> {
        let mut list = Command::new("git");
        list.arg("--no-pager")
            .arg("-C")
            .arg(repository)
            .arg("config");
        if let Some(file) = file {
            list.arg("--file").arg(file);
        }
        list.args([
            "--list",
            "-z",
            "--show-origin",
            "--show-scope",
            "--no-includes",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0");
        let listed = run_git_with(probe, list, limits)?;
        if listed.status.code() != Some(0) {
            return Err(unreadable_configuration());
        }
        config_entries(&listed.stdout).ok_or_else(unreadable_configuration)
    };
    let mut names = BTreeSet::new();
    let mut visited: BTreeSet<PathBuf> = BTreeSet::new();
    let listed = read(None)?;
    // A repository always has a `config` of its own, so its entries have the `local` scope. A
    // folder with none is not inside a repository: it has no configuration of its own to read,
    // and git run there starts nothing the folder names.
    let in_repository = listed.iter().any(|entry| entry.scope == "local");
    let mut pending: Vec<(ConfigEntry, usize)> = listed
        .into_iter()
        .filter(|entry| matches!(entry.scope.as_str(), "local" | "worktree"))
        .map(|entry| (entry, 0))
        .collect();
    // With `extensions.worktreeConfig`, each worktree has a `config.worktree` of its own that git
    // reads when it runs there, and `wt list` runs git in every worktree: the one of the
    // repository's main worktree and of every linked one is the repository's configuration too.
    if in_repository {
        for file in worktree_config_files(probe, repository, limits)? {
            if file.is_file() {
                pending.extend(read(Some(&file))?.into_iter().map(|entry| (entry, 0)));
            }
        }
    }
    while let Some((entry, depth)) = pending.pop() {
        if let Some(name) = driver_name(&entry.key) {
            if name.contains('=') {
                return Err(unreadable_configuration());
            }
            names.insert(name.to_owned());
        }
        let Some(target) = include_target(repository, &entry)? else {
            continue;
        };
        // Git itself stops at ten levels, with an error; a file already read is not read again.
        if depth >= INCLUDE_DEPTH {
            return Err(unreadable_configuration());
        }
        if visited.contains(&target) {
            continue;
        }
        // Past the cap the rest of the includes would go unread, and a driver in one of them
        // would run: there is no safe way on, so nothing is prepared.
        if visited.len() >= INCLUDE_FILES {
            return Err(unreadable_configuration());
        }
        visited.insert(target.clone());
        // A file that is not there is ignored by git too.
        if target.is_file() {
            pending.extend(
                read(Some(&target))?
                    .into_iter()
                    .map(|entry| (entry, depth + 1)),
            );
        }
    }
    Ok(names)
}

/// The `config.worktree` files that can exist: the main worktree's, in the common directory, and
/// one for each linked worktree under `worktrees/`. A file that is not there is skipped by the
/// caller; a directory that cannot be listed is an error.
fn worktree_config_files(probe: &Probe, repository: &Path, limits: Limits) -> Result<Vec<PathBuf>> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "--git-common-dir"])
        .env("GIT_OPTIONAL_LOCKS", "0");
    let output = run_git_with(probe, command, limits)?;
    if output.status.code() != Some(0) {
        return Err(unreadable_configuration());
    }
    let printed = std::str::from_utf8(&output.stdout).map_err(|_| unreadable_configuration())?;
    let printed = printed.trim_end_matches(['\n', '\r']);
    if printed.is_empty() {
        return Err(unreadable_configuration());
    }
    // Relative to the repository, or absolute (the join keeps an absolute path as it is).
    let common = repository.join(printed);
    let mut files = vec![common.join("config.worktree")];
    match fs::read_dir(common.join("worktrees")) {
        Ok(linked) => {
            for entry in linked {
                let entry = entry.map_err(|_| unreadable_configuration())?;
                files.push(entry.path().join("config.worktree"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::Read {
                program: "git".into(),
                source: error,
            });
        }
    }
    Ok(files)
}

const INCLUDE_DEPTH: usize = 10;
const INCLUDE_FILES: usize = 64;

/// One setting of `git config --list -z --show-origin --show-scope`.
#[derive(Debug, PartialEq, Eq)]
struct ConfigEntry {
    scope: String,
    /// `file:<path>` for a file; the path is relative to the repository for its own `config`.
    origin: String,
    /// Lower case in the section and the name, as git writes it; the subsection keeps its case.
    key: String,
    value: String,
}

/// The entries of `git config --list -z --show-origin --show-scope`: for each, the scope, the
/// origin and `key\nvalue` (a key with no value has no newline), each ended by a NUL.
fn config_entries(listing: &[u8]) -> Option<Vec<ConfigEntry>> {
    let text = std::str::from_utf8(listing).ok()?;
    let mut fields = text.split('\0');
    let mut entries = Vec::new();
    while let Some(scope) = fields.next().filter(|scope| !scope.is_empty()) {
        let origin = fields.next()?;
        let setting = fields.next()?;
        let (key, value) = setting.split_once('\n').unwrap_or((setting, ""));
        entries.push(ConfigEntry {
            scope: scope.to_owned(),
            origin: origin.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
        });
    }
    Some(entries)
}

/// The driver a `filter.<name>.<setting>` key names. A key with no subsection (`filter.x`) names
/// none.
fn driver_name(key: &str) -> Option<&str> {
    key.strip_prefix("filter.")
        .and_then(|rest| rest.rsplit_once('.'))
        .map(|(name, _)| name)
}

/// The file an `include.path` or `includeIf.<condition>.path` entry points to: a path relative
/// to the file that holds the entry, or one that starts with `~/`. `None` for an entry that
/// includes nothing. A value git would expand in a way this does not (`~user/`, a lone `~`,
/// `%(prefix)/`) or a `~/` with no home directory is an error: the file cannot be found, so it
/// cannot be read.
fn include_target(repository: &Path, entry: &ConfigEntry) -> Result<Option<PathBuf>> {
    let is_include = entry.key == "include.path"
        || (entry.key.starts_with("includeif.") && entry.key.ends_with(".path"));
    if !is_include || entry.value.is_empty() {
        return Ok(None);
    }
    let Some(holder) = entry.origin.strip_prefix("file:") else {
        return Err(unreadable_configuration());
    };
    let holder = repository.join(holder);
    let path = if let Some(rest) = entry.value.strip_prefix("~/") {
        let home = std::env::home_dir().ok_or_else(unreadable_configuration)?;
        home.join(rest)
    } else if entry.value.starts_with('~') || entry.value.starts_with("%(prefix)/") {
        return Err(unreadable_configuration());
    } else {
        let directory = holder.parent().ok_or_else(unreadable_configuration)?;
        directory.join(&entry.value)
    };
    Ok(Some(path))
}

/// A refusal to go on, as the error of a read that cannot be trusted.
fn refused(reason: &'static str) -> Error {
    Error::Read {
        program: "git".into(),
        source: io::Error::new(io::ErrorKind::InvalidData, reason),
    }
}

fn unreadable_configuration() -> Error {
    refused("the repository's filter drivers cannot be listed")
}

#[cfg(all(test, unix))]
mod tests;
