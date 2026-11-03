//! The harness's own CLI, run from the person's installation (ADR 0134).
//!
//! The Claude adapter drives a Claude Code binary that ships inside its package. That binary is
//! only as new as the adapter: it refuses a model released after it, and it behaves as the
//! release it is, not as the Claude Code the person runs in a terminal. The adapter runs the
//! binary that `CLAUDE_CODE_EXECUTABLE` names instead, so a pane runs the person's Claude Code:
//! the same version, the same models and the same automatic subagents.
//!
//! The program is found on the launch `PATH` (`environment::candidate_directories`, the same
//! `PATH` the person's login shell has) and used when it reports a version the models need.
//! Otherwise the adapter keeps its bundled binary, and the choice is logged either way.
//!
//! The version is asked of the program, which is an external command. It runs where a process
//! is sent to the supervisor (`Launch::send`), never under the Core lock (ADR 0079), and the
//! answer is remembered for the file it came from, so a program that did not change is not
//! asked again.

use super::{environment, registry};
use crate::streak::{self, Streak};
use soyeht_platform::log::{self, Failure, Line};
use soyeht_process::Limits;
use soyeht_ptyd::wire::{ProcessKind, Spawn};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant, SystemTime},
};

/// A program that is run for the first time after it changed can take seconds to start on a busy
/// host, which the system spends assessing it.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a program that did not say its version is left alone before it is asked again: a
/// start that timed out on a busy host is not held against the program for good, and a program
/// that never answers does not cost a timeout at every pane.
const ASK_AGAIN_AFTER: Duration = Duration::from_secs(30);
const VERSION_OUTPUT_BYTES: usize = 1024;

/// A program version, `major.minor.patch`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Version(u32, u32, u32);

impl Version {
    /// The version a program prints for `--version`: it leads the output (`2.1.312 (Claude
    /// Code)`).
    fn parse(output: &str) -> Option<Self> {
        let mut parts = output.split_whitespace().next()?.split('.');
        let version = Self(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        parts.next().is_none().then_some(version)
    }
}

/// A CLI that an adapter can be told to drive instead of the one it bundles.
pub(crate) struct UserCli {
    /// The adapter package that honours `variable`, by the name that precedes `@version` in its
    /// launch arguments.
    package: &'static str,
    /// The program name searched on the launch `PATH`.
    program: &'static str,
    /// The variable of the adapter's environment that names the program to run.
    variable: &'static str,
    /// The oldest version worth running: the first Claude Code that accepts the current models.
    /// 2.1.270 answers `400 ... does not support this model; version 2.1.280 or newer is
    /// required`. The CLI bundled with the pinned adapter is newer, so a program below this is
    /// the worse of the two.
    minimum: Version,
}

pub(crate) const CLAUDE: UserCli = UserCli {
    package: "@agentclientprotocol/claude-agent-acp",
    program: program_name(cfg!(windows)),
    variable: soyeht_ptyd::CLAUDE_CODE_EXECUTABLE,
    minimum: Version(2, 1, 280),
};

/// Windows runs a `.cmd` shim through a shell, which Node cannot start without one, so the
/// adapter is only given the program itself.
const fn program_name(windows: bool) -> &'static str {
    if windows { "claude.exe" } else { "claude" }
}

/// Why a pane runs the adapter's bundled CLI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Why {
    /// The person has no such program on the launch `PATH`.
    NotInstalled,
    /// The program is older than the CLI's `minimum`.
    TooOld,
    /// The program did not say its version.
    VersionUnknown,
}

impl Why {
    fn kind(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::TooOld => "too_old",
            Self::VersionUnknown => "version_unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Choice {
    /// The person's program, by its canonical path.
    Own(PathBuf),
    Bundled(Why),
}

/// The CLI to run: the person's `found` program when `version` says it is new enough.
fn decide(
    minimum: Version,
    found: Option<PathBuf>,
    version: impl FnOnce(&Path) -> Option<Version>,
) -> Choice {
    let Some(path) = found else {
        return Choice::Bundled(Why::NotInstalled);
    };
    match version(&path) {
        Some(version) if version >= minimum => Choice::Own(path),
        Some(_) => Choice::Bundled(Why::TooOld),
        None => Choice::Bundled(Why::VersionUnknown),
    }
}

/// What identifies a program file across runs: it changes when the program is replaced.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Stamp(Option<SystemTime>, u64);

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        match path.metadata() {
            // ignored: a file system without modification times tells programs apart by size.
            Ok(metadata) => Some(Self(metadata.modified().ok(), metadata.len())),
            Err(error) => {
                log::failure("engine.agent_cli_stat_failed", &error);
                None
            }
        }
    }
}

/// The version a program reported, for the file it came from.
struct Asked {
    path: PathBuf,
    stamp: Stamp,
    /// `None` when the program did not say.
    version: Option<Version>,
    at: Instant,
}

/// What the engine remembers of one CLI.
#[derive(Default)]
struct Memory {
    asked: Option<Asked>,
    /// The latest choice, so that the log says it at the first spawn and at each change.
    announced: Option<Choice>,
}

impl Memory {
    /// The version of the program at `path`: the one it reported for this very file, else what
    /// `ask` says. An answer is kept until the file changes; the lack of one, for
    /// [`ASK_AGAIN_AFTER`].
    fn version_of(
        &mut self,
        path: &Path,
        stamp: Stamp,
        now: Instant,
        ask: impl FnOnce() -> Option<Version>,
    ) -> Option<Version> {
        if let Some(asked) = &self.asked
            && asked.path == path
            && asked.stamp == stamp
            && (asked.version.is_some() || now.duration_since(asked.at) < ASK_AGAIN_AFTER)
        {
            return asked.version;
        }
        let version = ask();
        self.asked = Some(Asked {
            path: path.to_owned(),
            stamp,
            version,
            at: now,
        });
        version
    }
}

static MEMORY: Mutex<BTreeMap<&'static str, Memory>> = Mutex::new(BTreeMap::new());

fn memory() -> MutexGuard<'static, BTreeMap<&'static str, Memory>> {
    // A holder that panicked left whole entries behind: each update is one assignment.
    MEMORY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `path --version`. Off the Core lock only: `soyeht_process::run` refuses otherwise.
fn ask_version(cli: &UserCli, path: &Path) -> Option<Version> {
    let mut command = Command::new(path);
    command.arg("--version");
    match soyeht_process::run(command, Limits::new(VERSION_TIMEOUT, VERSION_OUTPUT_BYTES)) {
        Ok(output) if output.success() => Version::parse(&String::from_utf8_lossy(&output.stdout)),
        Ok(_) => {
            Line::new("engine.agent_cli_version_failed")
                .field("cli", cli.variable)
                .field("kind", "exit_status")
                .emit();
            None
        }
        Err(error) => {
            log::failure_with(
                "engine.agent_cli_version_failed",
                &[("cli", cli.variable)],
                &error,
            );
            None
        }
    }
}

/// The CLI a process of `cli`'s adapter should run now. Off the Core lock.
pub(crate) fn settle(cli: &UserCli) -> Choice {
    let found = registry::resolve_executable(cli.program, environment::candidate_directories(&[]))
        // The variable is a string of the child's environment.
        .filter(|path| path.to_str().is_some());
    let (choice, announce) = {
        let mut all = memory();
        let memory = all.entry(cli.variable).or_default();
        let choice = decide(cli.minimum, found, |path| {
            let stamp = Stamp::of(path)?;
            memory.version_of(path, stamp, Instant::now(), || ask_version(cli, path))
        });
        let announce = memory.announced.as_ref() != Some(&choice);
        memory.announced = Some(choice.clone());
        (choice, announce)
    };
    if announce {
        let line = Line::new("engine.agent_cli_chosen").field("cli", cli.variable);
        match &choice {
            Choice::Own(_) => line.field("choice", "own"),
            Choice::Bundled(why) => line.field("choice", "bundled").field("why", why.kind()),
        }
        .emit();
    }
    choice
}

/// The CLI whose adapter `argv` launches, if one is declared above.
fn cli_of(argv: &[String]) -> Option<&'static UserCli> {
    argv.iter()
        .any(|argument| {
            argument
                .strip_prefix(CLAUDE.package)
                .is_some_and(|rest| rest.starts_with('@'))
        })
        .then_some(&CLAUDE)
}

/// Gives the adapter in `spawn` the person's CLI to drive, when it has one, and says whether it
/// did. A launch that sets the variable itself (a custom entry's environment) keeps its own
/// choice. Off the Core lock.
pub(crate) fn complete(spawn: &mut Spawn) -> bool {
    if spawn.kind != ProcessKind::PipesAcp {
        return false;
    }
    let Some(cli) = cli_of(&spawn.argv) else {
        return false;
    };
    if spawn.env.contains_key(cli.variable) {
        return false;
    }
    if let Choice::Own(path) = settle(cli)
        && let Some(path) = path.to_str()
    {
        spawn.env.insert(cli.variable.into(), path.into());
        return true;
    }
    false
}

/// `spawn` as it was before [`complete`] gave it a CLI.
pub(crate) fn without_cli(spawn: &Spawn) -> Spawn {
    let mut plain = spawn.clone();
    plain.env.remove(CLAUDE.variable);
    plain
}

static REFUSALS: Mutex<Streak> = Mutex::new(Streak::once());

/// The supervisor refused a process that [`complete`] gave a CLI, as a supervisor that outlived
/// an update and does not know the variable does: the process starts without it. Logged at the
/// first refusal of a streak.
pub(crate) fn refused(error: &crate::errors::Error) {
    if streak::first_failure(&REFUSALS, &error.failure_kind()) {
        log::failure("engine.agent_cli_refused", error);
    }
}

/// The supervisor took a process that [`complete`] gave a CLI.
pub(crate) fn accepted() {
    streak::recovered(&REFUSALS);
}

/// Asks for the version in the background, so the first agent pane does not wait for it.
pub(crate) fn warm() {
    let started = thread::Builder::new().name("user-cli".into()).spawn(|| {
        settle(&CLAUDE);
    });
    if let Err(error) = started {
        log::failure("engine.agent_cli_warm_failed", &error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const MINIMUM: Version = Version(2, 1, 280);

    fn path() -> PathBuf {
        PathBuf::from("/home/person/bin/claude")
    }

    #[test]
    fn a_program_that_is_new_enough_is_the_one_the_adapter_drives() {
        for version in [Version(2, 1, 280), Version(2, 1, 312), Version(3, 0, 0)] {
            assert_eq!(
                decide(MINIMUM, Some(path()), |_| Some(version)),
                Choice::Own(path())
            );
        }
    }

    #[test]
    fn a_program_that_is_too_old_leaves_the_adapter_its_own() {
        for version in [Version(2, 1, 270), Version(2, 1, 279), Version(1, 99, 999)] {
            assert_eq!(
                decide(MINIMUM, Some(path()), |_| Some(version)),
                Choice::Bundled(Why::TooOld)
            );
        }
    }

    #[test]
    fn no_program_asks_nothing_and_leaves_the_adapter_its_own() {
        let asked = Cell::new(false);
        let choice = decide(MINIMUM, None, |_| {
            asked.set(true);
            Some(Version(9, 9, 9))
        });
        assert_eq!(choice, Choice::Bundled(Why::NotInstalled));
        assert!(!asked.get());
    }

    #[test]
    fn a_program_that_does_not_say_its_version_is_not_trusted() {
        assert_eq!(
            decide(MINIMUM, Some(path()), |_| None),
            Choice::Bundled(Why::VersionUnknown)
        );
    }

    #[test]
    fn the_version_leads_the_output_of_the_program() {
        assert_eq!(
            Version::parse("2.1.312 (Claude Code)\n"),
            Some(Version(2, 1, 312))
        );
        assert_eq!(Version::parse("2.1.0"), Some(Version(2, 1, 0)));
        for refused in ["", "claude", "2.1", "2.1.x", "2.1.3.4", "v2.1.3", "-1.2.3"] {
            assert_eq!(Version::parse(refused), None, "{refused:?}");
        }
        assert!(Version(2, 1, 312) > Version(2, 1, 99));
        assert!(Version(10, 0, 0) > Version(9, 9, 9));
    }

    #[test]
    fn windows_names_the_program_itself_and_not_a_shell_shim() {
        assert_eq!(program_name(true), "claude.exe");
        assert_eq!(program_name(false), "claude");
        assert_eq!(
            CLAUDE.program,
            if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_shim_is_not_found_where_only_the_program_would_be() {
        let directory = std::env::temp_dir().join(format!("soyeht-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("claude.cmd"), "@echo off").unwrap();
        let find = || registry::resolve_executable(CLAUDE.program, [directory.clone()]);
        assert_eq!(find(), None);
        std::fs::write(directory.join("claude.exe"), "MZ").unwrap();
        assert!(find().is_some_and(|path| path.ends_with("claude.exe")));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_version_is_kept_for_its_file_and_a_missing_one_is_asked_for_again_later() {
        let (file, other) = (Stamp(None, 10), Stamp(None, 11));
        let (start, asked) = (Instant::now(), Cell::new(0));
        let mut memory = Memory::default();
        let mut version_at = |stamp: &Stamp, after: u64, says: Option<Version>| {
            memory.version_of(
                &path(),
                stamp.clone(),
                start + Duration::from_secs(after),
                || {
                    asked.set(asked.get() + 1);
                    says
                },
            )
        };
        // An answer stays while the file does.
        assert_eq!(
            version_at(&file, 0, Some(Version(2, 1, 312))),
            Some(Version(2, 1, 312))
        );
        assert_eq!(version_at(&file, 3600, None), Some(Version(2, 1, 312)));
        assert_eq!(asked.get(), 1);
        // A file that changed is asked again.
        assert_eq!(version_at(&other, 3601, None), None);
        assert_eq!(asked.get(), 2);
        // No answer is not held against the program for good, nor asked at every pane.
        assert_eq!(version_at(&other, 3602, Some(Version(2, 1, 400))), None);
        assert_eq!(asked.get(), 2);
        let later = 3601 + ASK_AGAIN_AFTER.as_secs();
        assert_eq!(
            version_at(&other, later, Some(Version(2, 1, 400))),
            Some(Version(2, 1, 400))
        );
        assert_eq!(asked.get(), 3);
    }

    #[test]
    fn the_spawn_without_the_cli_is_the_one_before_it_was_given() {
        let plain = adapter(
            "@agentclientprotocol/claude-agent-acp@0.100.0",
            ProcessKind::PipesAcp,
        );
        let mut given = plain.clone();
        given
            .env
            .insert(CLAUDE.variable.into(), "/home/person/bin/claude".into());
        assert_eq!(without_cli(&given), plain);
    }

    fn adapter(package: &str, kind: ProcessKind) -> Spawn {
        Spawn {
            instance_key: "key".into(),
            generation: 1,
            kind,
            cwd: "/".into(),
            argv: vec!["/bin/npx".into(), "--yes".into(), package.into()],
            env: Default::default(),
            cols: 80,
            rows: 24,
        }
    }

    #[test]
    fn only_the_claude_adapter_is_given_the_variable() {
        assert!(
            cli_of(
                &adapter(
                    "@agentclientprotocol/claude-agent-acp@0.100.0",
                    ProcessKind::PipesAcp
                )
                .argv
            )
            .is_some()
        );
        for other in [
            "@agentclientprotocol/codex-acp@1.11.0",
            "@agentclientprotocol/claude-agent-acp-extra@1.0.0",
            "@agentclientprotocol/claude-agent-acp",
        ] {
            assert!(
                cli_of(&adapter(other, ProcessKind::PipesAcp).argv).is_none(),
                "{other}"
            );
        }
    }

    #[test]
    fn a_process_that_is_not_an_adapter_or_names_its_own_cli_is_left_alone() {
        let package = "@agentclientprotocol/claude-agent-acp@0.100.0";
        let mut terminal = adapter(package, ProcessKind::Pty);
        assert!(!complete(&mut terminal));
        assert!(terminal.env.is_empty());

        let mut recipe = adapter(package, ProcessKind::PipesAcp);
        recipe
            .env
            .insert(CLAUDE.variable.into(), "/recipe/claude".into());
        assert!(!complete(&mut recipe));
        assert_eq!(recipe.env[CLAUDE.variable], "/recipe/claude");
    }
}
