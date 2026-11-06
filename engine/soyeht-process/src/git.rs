//! Starting `git` without opening the macOS installer. See [`soyeht_platform::git`].

use crate::{Error, Limits, Output, Result, run};
use soyeht_platform::git::{Availability, Probe};
use std::{
    collections::BTreeSet,
    io,
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
/// turns the monitor off and clears the programs of each driver the configuration names (the
/// names are listed first, with a command that starts nothing). Textconv and external diff
/// programs are not in this list: they are off with `--no-textconv` and `--no-ext-diff` on the
/// commands that have them, which the caller passes. A submodule is a repository with its own
/// configuration, which a command that looks inside it (`diff` and `status` do) reads with its
/// own filters: the caller passes `--ignore-submodules=dirty` so that it does not look.
///
/// It also never pages, takes no optional lock and reads paths literally. A configuration whose
/// driver names cannot be listed or cannot be written as `-c` settings is an error: nothing is
/// prepared.
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
        let mut list = Command::new("git");
        list.arg("--no-pager")
            .arg("-C")
            .arg(repository)
            .args(["config", "-z", "--name-only", "--get-regexp", "^filter\\."])
            .env("GIT_OPTIONAL_LOCKS", "0");
        let listed = run_git_with(probe, list, limits)?;
        // 1 is "no such key": the repository has no filter drivers.
        let names = match listed.status.code() {
            Some(0) => filter_names(&listed.stdout),
            Some(1) => Some(BTreeSet::new()),
            _ => None,
        };
        let Some(names) = names else {
            return Err(unreadable_configuration());
        };
        let mut settings = vec!["core.fsmonitor=false".to_owned()];
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

fn unreadable_configuration() -> Error {
    Error::Read {
        program: "git".into(),
        source: io::Error::new(
            io::ErrorKind::InvalidData,
            "the repository's filter drivers cannot be listed",
        ),
    }
}

/// The driver names in the NUL separated keys `filter.<name>.<setting>` of `git config -z
/// --name-only`. `None` for a name `-c` cannot carry (it has an `=`, which would end the key) or
/// for output that is not text.
fn filter_names(listing: &[u8]) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for key in listing
        .split(|&byte| byte == 0)
        .filter(|key| !key.is_empty())
    {
        let key = std::str::from_utf8(key).ok()?;
        // A key with no subsection (`filter.x`) names no driver.
        let Some((name, _)) = key
            .strip_prefix("filter.")
            .and_then(|rest| rest.rsplit_once('.'))
        else {
            continue;
        };
        if name.contains('=') {
            return None;
        }
        names.insert(name.to_owned());
    }
    Some(names)
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
    fn filter_drivers_are_listed_by_name_from_their_keys() {
        let names =
            filter_names(b"filter.lfs.clean\0filter.lfs.smudge\0filter.a.b.process\0filter.x\0");
        assert_eq!(
            names.unwrap().into_iter().collect::<Vec<_>>(),
            ["a.b", "lfs"]
        );
        assert!(filter_names(b"").unwrap().is_empty());
        // A name `-c` could not carry, and output that is not text, are refused.
        assert!(filter_names(b"filter.a=b.clean\0").is_none());
        assert!(filter_names(b"filter.\xff.clean\0").is_none());
    }

    /// A repository whose own configuration names a program for each way git starts one while it
    /// reads: the file system monitor and the three programs of a filter driver. Each program
    /// leaves a file named for it in `root`.
    fn repository_with_programs(label: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("soyeht-read-only-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let git = |arguments: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(arguments)
                .status()
                .unwrap();
            assert!(status.success(), "{arguments:?}");
        };
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
}
