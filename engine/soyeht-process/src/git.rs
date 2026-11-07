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
    let mut pending: Vec<(ConfigEntry, usize)> = read(None)?
        .into_iter()
        .filter(|entry| matches!(entry.scope.as_str(), "local" | "worktree"))
        .map(|entry| (entry, 0))
        .collect();
    // With `extensions.worktreeConfig`, each worktree has a `config.worktree` of its own that git
    // reads when it runs there, and `wt list` runs git in every worktree: the one of the
    // repository's main worktree and of every linked one is the repository's configuration too.
    for file in worktree_config_files(probe, repository, limits)? {
        if file.is_file() {
            pending.extend(read(Some(&file))?.into_iter().map(|entry| (entry, 0)));
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
        Err(_) => return Err(unreadable_configuration()),
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
mod tests {
    use super::*;
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::Duration,
    };

    /// A machine whose `git` is a launcher that records that it ran. The launcher stands for
    /// the macOS stub that opens the installer.
    struct Launcher {
        root: PathBuf,
    }

    impl Launcher {
        fn new(label: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let root =
                env::temp_dir().join(format!("soyeht-run-git-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("usr/bin")).unwrap();
            let script = format!("#!/bin/sh\ntouch '{}'\n", root.join("ran").display());
            let launcher = root.join("usr/bin/git");
            fs::write(&launcher, script).unwrap();
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
            Self { root }
        }

        fn probe(&self) -> Probe {
            Probe {
                launcher: Some(self.root.join("usr/bin/git")),
                path: self.root.join("usr/bin").into_os_string(),
                developer_dir: None,
                select_link: self.root.join("xcode_select_link"),
                default_tools: self.root.join("Library/CommandLineTools"),
            }
        }

        fn ran(&self) -> bool {
            self.root.join("ran").exists()
        }
    }

    impl Drop for Launcher {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn git_is_not_started_when_the_developer_tools_are_missing() {
        let machine = Launcher::new("missing");
        let limits = Limits::new(Duration::from_secs(10), 8192);
        let error = run_git_with(&machine.probe(), Command::new("git"), limits).unwrap_err();
        assert!(matches!(error, Error::GitUnavailable), "{error}");
        assert!(matches!(
            require_git_with(&machine.probe()),
            Err(Error::GitUnavailable)
        ));
        assert!(!machine.ran(), "the launcher must never start");
    }

    #[test]
    fn git_starts_when_the_developer_tools_are_present() {
        use std::os::unix::fs::PermissionsExt;
        let machine = Launcher::new("present");
        let real = machine.root.join("Library/CommandLineTools/usr/bin/git");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new(machine.root.join("usr/bin/git"));
        command.arg("--version");
        let limits = Limits::new(Duration::from_secs(10), 8192);
        run_git_with(&machine.probe(), command, limits).unwrap();
        assert!(machine.ran());
    }

    #[test]
    fn only_the_repositorys_own_entries_name_drivers_and_includes() {
        let listing = b"global\0file:/home/p/.gitconfig\0filter.lfs.clean\ngit-lfs clean\0\
            local\0file:.git/config\0filter.a.b.process\ncat\0\
            local\0file:.git/config\0filter.x\0\
            local\0file:.git/config\0include.path\n../extra.cfg\0\
            local\0file:.git/config\0includeif.gitdir:/r/**.path\n~/more.cfg\0";
        let entries = config_entries(listing).unwrap();
        assert_eq!(entries.len(), 5);
        let own: Vec<_> = entries
            .iter()
            .filter(|entry| entry.scope == "local")
            .collect();
        let names: Vec<_> = own
            .iter()
            .filter_map(|entry| driver_name(&entry.key))
            .collect();
        assert_eq!(names, ["a.b"], "a key with no subsection names none");
        let repository = Path::new("/work/repository");
        let targets: Vec<_> = own
            .iter()
            .map(|entry| include_target(repository, entry).unwrap())
            .collect();
        let targets: Vec<_> = targets.into_iter().flatten().collect();
        assert_eq!(targets[0], Path::new("/work/repository/.git/../extra.cfg"));
        assert_eq!(targets[1], std::env::home_dir().unwrap().join("more.cfg"));
        // Output that is not text, or that stops in the middle of an entry, is refused.
        assert!(config_entries(b"local\0file:x\0\xff\0").is_none());
        assert!(config_entries(b"local\0file:x").is_none());
        assert_eq!(config_entries(b"").unwrap(), []);
    }

    #[test]
    fn the_version_of_git_is_read_from_its_first_line() {
        assert_eq!(
            git_version("git version 2.50.1 (Apple Git-155)\n"),
            Some((2, 50))
        );
        assert_eq!(git_version("git version 2.25.1"), Some((2, 25)));
        assert_eq!(git_version("git version 2.31.0.windows.1"), Some((2, 31)));
        assert_eq!(git_version("something else"), None);
    }

    /// `git` in `root` with an identity, for a test's setup. Its standard input is closed: a
    /// program a configuration names reads it.
    fn git_in(root: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{arguments:?}");
    }

    /// Adds the worktree `name` to the repository at `root`, with the settings of
    /// [`ReadOnlyGit::harden_environment`] when `hardened`.
    fn add_worktree(root: &Path, name: &str, hardened: bool) {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(root)
            .args(["worktree", "add", "-q", "-b", name])
            .arg(root.join(name));
        if hardened {
            let limits = Limits::new(Duration::from_secs(30), 1 << 20);
            ReadOnlyGit::prepare(root, limits)
                .unwrap()
                .harden_environment(&mut command)
                .unwrap();
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A repository whose own configuration names a program for each way git starts one while it
    /// reads: the file system monitor and the three programs of a filter driver. Each program
    /// leaves a file named for it in `root`.
    fn repository_with_programs(label: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("soyeht-read-only-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let git = |arguments: &[&str]| git_in(&root, arguments);
        git(&["init", "-q"]);
        fs::write(root.join(".gitattributes"), "*.bin filter=probe\n").unwrap();
        fs::write(root.join("a.bin"), "one\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "baseline"]);
        let program =
            |name: &str| format!("sh -c 'touch \"{}/ran-{name}\"; cat' --", root.display());
        for (key, name) in [
            ("filter.probe.clean", "clean"),
            ("filter.probe.smudge", "smudge"),
            ("core.fsmonitor", "fsmonitor"),
        ] {
            git(&["config", key, &program(name)]);
        }
        git(&["config", "filter.probe.process", &program("process")]);
        // A driver name with dots is listed whole.
        git(&["config", "filter.dotted.name.clean", &program("dotted")]);
        fs::write(root.join("a.bin"), "two\n").unwrap();
        fs::write(root.join("untracked.txt"), "new\n").unwrap();
        root
    }

    fn programs_that_ran(root: &Path) -> Vec<String> {
        let mut ran: Vec<String> = fs::read_dir(root)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("ran-"))
            .collect();
        ran.sort();
        ran
    }

    #[test]
    fn a_read_only_command_starts_none_of_the_programs_a_repository_names() {
        let limits = Limits::new(Duration::from_secs(30), 1 << 20);
        let root = repository_with_programs("commands");
        let commands: [&[&str]; 4] = [
            &[
                "diff",
                "--no-ext-diff",
                "--name-only",
                "-z",
                "HEAD",
                "--",
                ".",
            ],
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--unified=200",
                "HEAD",
                "--",
                "a.bin",
            ],
            &["ls-files", "--others", "--exclude-standard", "-z"],
            &["status", "--porcelain"],
        ];

        // The same commands without the hardening do start them, so the check below can fail.
        for arguments in commands {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(arguments)
                .output()
                .unwrap();
            assert!(status.status.success(), "{arguments:?}");
        }
        assert!(
            !programs_that_ran(&root).is_empty(),
            "the control started nothing"
        );
        for entry in programs_that_ran(&root) {
            fs::remove_file(root.join(entry)).unwrap();
        }

        for arguments in commands {
            let mut command = ReadOnlyGit::prepare(&root, limits).unwrap().command();
            command.args(arguments);
            let output = run_git(command, limits).unwrap();
            assert!(output.success(), "{arguments:?}: {}", output.stderr_text());
        }
        let ran = programs_that_ran(&root);
        fs::remove_dir_all(&root).unwrap();
        assert!(ran.is_empty(), "{ran:?}");
    }

    /// Worktrunk runs git itself, so the settings must reach the git it starts through the
    /// environment. The same checkout without them runs the repository's hook and programs.
    #[test]
    fn the_environment_carries_the_settings_to_a_git_another_program_starts() {
        let limits = Limits::new(Duration::from_secs(30), 1 << 20);
        let root = repository_with_programs("environment");
        let hook = root.join(".git/hooks/post-checkout");
        fs::write(
            &hook,
            format!("#!/bin/sh\ntouch \"{}/ran-hook\"\n", root.display()),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        add_worktree(&root, "control", false);
        assert!(
            programs_that_ran(&root).contains(&"ran-hook".to_owned()),
            "the control ran no hook"
        );
        for entry in programs_that_ran(&root) {
            fs::remove_file(root.join(entry)).unwrap();
        }
        add_worktree(&root, "hardened", true);
        let mut status = Command::new("git");
        status.arg("-C").arg(&root).args(["status", "--porcelain"]);
        ReadOnlyGit::prepare(&root, limits)
            .unwrap()
            .harden_environment(&mut status)
            .unwrap();
        assert!(status.output().unwrap().status.success());
        let ran = programs_that_ran(&root);
        fs::remove_dir_all(&root).unwrap();
        assert!(ran.is_empty(), "{ran:?}");
    }

    /// A driver defined in a file the configuration includes, under a condition that holds only
    /// inside a worktree that does not exist yet, is cleared for that worktree too. The control
    /// shows the driver does run in the worktree without the settings.
    #[test]
    fn a_driver_of_an_included_file_is_cleared_whatever_the_condition_of_the_include() {
        let root = fs::canonicalize(repository_with_programs("included")).unwrap();
        let git = |arguments: &[&str]| git_in(&root, arguments);
        // Each driver is a script beside the repository's files: a program with a `;` in a
        // config file would be read as a comment.
        for name in ["late", "deep"] {
            let script = root.join(format!("{name}.sh"));
            fs::write(
                &script,
                format!("#!/bin/sh\ntouch '{}/ran-{name}'\ncat\n", root.display()),
            )
            .unwrap();
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        fs::write(
            root.join("extra.cfg"),
            format!(
                "[filter \"late\"]\n\tsmudge = {}/late.sh\n[include]\n\tpath = deeper.cfg\n",
                root.display()
            ),
        )
        .unwrap();
        fs::write(
            root.join("deeper.cfg"),
            format!("[filter \"deep\"]\n\tsmudge = {}/deep.sh\n", root.display()),
        )
        .unwrap();
        git(&[
            "config",
            &format!("includeIf.gitdir:{}/.git/worktrees/**.path", root.display()),
            "../extra.cfg",
        ]);
        fs::write(
            root.join(".gitattributes"),
            "*.late filter=late\n*.deep filter=deep\n",
        )
        .unwrap();
        fs::write(root.join("a.late"), "one\n").unwrap();
        fs::write(root.join("a.deep"), "one\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "included"]);
        for entry in programs_that_ran(&root) {
            fs::remove_file(root.join(entry)).unwrap();
        }
        add_worktree(&root, "control", false);
        let started = programs_that_ran(&root);
        for name in ["ran-late", "ran-deep"] {
            assert!(
                started.contains(&name.to_owned()),
                "the control did not run {name}: {started:?}"
            );
        }
        for entry in started {
            fs::remove_file(root.join(entry)).unwrap();
        }
        add_worktree(&root, "hardened", true);
        let ran = programs_that_ran(&root);
        fs::remove_dir_all(&root).unwrap();
        assert!(ran.is_empty(), "{ran:?}");
    }

    /// The person's own drivers (git-lfs) are not the repository's: only the repository's are
    /// cleared.
    #[test]
    fn a_repository_without_drivers_of_its_own_clears_none() {
        let limits = Limits::new(Duration::from_secs(30), 1 << 20);
        let root = env::temp_dir().join(format!("soyeht-read-only-plain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        let git = ReadOnlyGit::prepare(&root, limits).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            git.settings
                .iter()
                .filter(|setting| setting.starts_with("filter."))
                .count(),
            0,
            "{:?}",
            git.settings
        );
    }

    #[test]
    fn the_settings_come_after_the_ones_the_environment_carries() {
        let git = ReadOnlyGit {
            repository: PathBuf::from("/r"),
            settings: vec!["core.fsmonitor=false".to_owned()],
        };
        let mut command = Command::new("true");
        git.harden_environment(&mut command).unwrap();
        // What the process itself carries comes first; the settings are numbered after it.
        let value = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
        };
        let inherited: usize = std::env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        assert_eq!(value("GIT_CONFIG_COUNT"), Some((inherited + 1).to_string()));
        assert_eq!(
            value(&format!("GIT_CONFIG_KEY_{inherited}")).as_deref(),
            Some("core.fsmonitor")
        );
    }

    /// A repository with one commit and no drivers, for the tests of what the configuration lists.
    fn plain_repository(label: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("soyeht-read-only-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(&root).unwrap();
        git_in(&root, &["init", "-q"]);
        git_in(&root, &["commit", "-q", "--allow-empty", "-m", "baseline"]);
        root
    }

    fn append(file: &Path, text: &str) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new().append(true).open(file).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn cleared(git: &ReadOnlyGit, driver: &str) -> bool {
        git.settings.contains(&format!("filter.{driver}.clean="))
    }

    fn lists() -> Limits {
        Limits::new(Duration::from_secs(30), 1 << 20)
    }

    fn includes(count: usize) -> String {
        (0..count)
            .map(|number| format!("[include]\n\tpath = inc{number}.cfg\n"))
            .collect()
    }

    /// Past the cap the includes that are left would go unread, and a driver in one of them
    /// would run: preparing fails, and a configuration at the cap still works.
    #[test]
    fn more_includes_than_the_cap_fail_instead_of_being_dropped() {
        let root = plain_repository("include-cap");
        let config = root.join(".git/config");
        // The driver is in the first file the walk would drop: it pops the last entry first.
        fs::write(
            root.join(".git/inc1.cfg"),
            "[filter \"late\"]\n\tclean = touch /nonexistent\n",
        )
        .unwrap();
        append(&config, &includes(INCLUDE_FILES));
        let at_the_cap = ReadOnlyGit::prepare(&root, lists()).unwrap();
        assert!(cleared(&at_the_cap, "late"));
        append(&config, "[include]\n\tpath = one-too-many.cfg\n");
        let over = ReadOnlyGit::prepare(&root, lists());
        fs::remove_dir_all(&root).unwrap();
        assert!(over.is_err(), "65 includes were prepared");
    }

    /// With `extensions.worktreeConfig`, the `config.worktree` of the main worktree and of every
    /// linked one is read by the git that `wt` starts in it, so its drivers are cleared whichever
    /// worktree the repository is prepared from, and so are the ones in files it includes.
    #[test]
    fn the_worktree_config_of_every_worktree_is_read() {
        let root = plain_repository("worktree-config");
        git_in(&root, &["config", "extensions.worktreeConfig", "true"]);
        git_in(
            &root,
            &["worktree", "add", "-q", "-b", "first", "../first-worktree"],
        );
        git_in(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "second",
                "../second-worktree",
            ],
        );
        let common = root.join(".git");
        fs::write(
            common.join("worktrees/first-worktree/config.worktree"),
            "[filter \"linked\"]\n\tclean = x\n[include]\n\tpath = more.cfg\n",
        )
        .unwrap();
        fs::write(
            common.join("worktrees/first-worktree/more.cfg"),
            "[filter \"included\"]\n\tclean = x\n",
        )
        .unwrap();
        fs::write(
            common.join("config.worktree"),
            "[filter \"main\"]\n\tclean = x\n",
        )
        .unwrap();
        let second = root.with_file_name("second-worktree");
        let from_main = ReadOnlyGit::prepare(&root, lists()).unwrap();
        let from_other = ReadOnlyGit::prepare(&second, lists()).unwrap();
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&second).unwrap();
        let _ = fs::remove_dir_all(root.with_file_name("first-worktree"));
        for git in [&from_main, &from_other] {
            for driver in ["linked", "included", "main"] {
                assert!(cleared(git, driver), "{driver}: {:?}", git.settings);
            }
        }
    }

    /// A `config.worktree` that git cannot read is not skipped.
    #[test]
    fn an_unreadable_worktree_config_fails() {
        let root = plain_repository("worktree-config-bad");
        fs::write(root.join(".git/config.worktree"), "[filter\n").unwrap();
        let prepared = ReadOnlyGit::prepare(&root, lists());
        fs::remove_dir_all(&root).unwrap();
        assert!(prepared.is_err());
    }

    /// Git expands `~user/` to that user's home; this cannot, so an include that starts with a
    /// `~` that is not `~/` fails instead of being read as a relative path.
    #[test]
    fn an_include_git_would_expand_differently_fails() {
        let entry = |value: &str| ConfigEntry {
            scope: "local".into(),
            origin: "file:.git/config".into(),
            key: "include.path".into(),
            value: value.into(),
        };
        let repository = Path::new("/r");
        for value in ["~someone/x.cfg", "~", "%(prefix)/x.cfg"] {
            assert!(
                include_target(repository, &entry(value)).is_err(),
                "{value}"
            );
        }
        assert!(include_target(repository, &entry("~/x.cfg")).is_ok());
        assert!(include_target(repository, &entry("x.cfg")).is_ok());

        let root = plain_repository("tilde-user");
        append(
            &root.join(".git/config"),
            "[include]\n\tpath = ~someone/x.cfg\n",
        );
        let prepared = ReadOnlyGit::prepare(&root, lists());
        fs::remove_dir_all(&root).unwrap();
        assert!(prepared.is_err());
    }

    #[test]
    fn a_count_in_the_environment_that_cannot_be_added_to_fails() {
        let git = ReadOnlyGit {
            repository: PathBuf::from("/r"),
            settings: vec!["a=b".to_owned(), "c=d".to_owned()],
        };
        let hardened = |inherited: Option<&str>| {
            let mut command = Command::new("true");
            let result = git.harden_over(&mut command, inherited);
            (result, command.get_envs().count())
        };
        for bad in [
            "many",
            "",
            "-1",
            &usize::MAX.to_string(),
            &(usize::MAX - 1).to_string(),
        ] {
            let (result, touched) = hardened(Some(bad));
            assert!(result.is_err(), "{bad}");
            assert_eq!(touched, 0, "{bad}: the command was changed");
        }
        let (result, _) = hardened(Some("3"));
        assert!(result.is_ok());
        assert!(hardened(None).0.is_ok());
    }
}
