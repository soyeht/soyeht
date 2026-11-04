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
//! is sent to the supervisor, never under the Core lock (ADR 0079), and the answer is remembered
//! for the file it came from, so a program that did not change is not asked again. Every send of
//! an adapter process goes through [`prepare`] and [`Given::send`]: a pane's process
//! (`Launch::send`) and the candidate of a harness switch.

use super::{environment, pinned_adapter, registry};
use crate::{
    errors::Error,
    streak::{self, Streak},
    supervisor_handle::Supervisor,
};
use soyeht_platform::log::{self, Failure, Line};
use soyeht_process::Limits;
use soyeht_ptyd::{
    wire::{ProcessKind, Request, Response, Spawn},
    wire_kind,
};
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
    /// Code)`). A pre-release or build suffix (`2.1.313-beta.1`, `2.1.3+build`) counts as the
    /// release it precedes or builds: a person on a pre-release channel is the one who wants
    /// their own CLI, and the numbers are what the models need. Anything else is no version.
    fn parse(output: &str) -> Option<Self> {
        let word = output.split_whitespace().next()?;
        let numbers = word
            .split_once(['-', '+'])
            .map_or(word, |(numbers, _)| numbers);
        let mut parts = numbers.split('.');
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
    /// The built-in harness whose adapter honours `variable`. Its pinned package, in the registry
    /// snapshot, is what identifies the adapter in a launch.
    harness: &'static str,
    /// The program name searched on the launch `PATH`.
    program: &'static str,
    /// The variable of the adapter's environment that names the program to run.
    variable: &'static str,
    /// The oldest version worth running. It is decided against one adapter release (below) and
    /// holds only for that one.
    minimum: Version,
    /// The pinned adapter version `minimum` was decided against. A test fails when the snapshot
    /// pins another, so a bump of the pin makes someone read the adapter again.
    #[cfg(test)]
    decided_for: &'static str,
}

/// The CLIs the engine can give an adapter.
static CLIS: &[UserCli] = &[CLAUDE];

/// Claude Code. The adapter 0.100.0 ships with SDK 0.3.316, whose bundled CLI is 2.1.316, and is
/// written against CLIs from 2.1.206 on. 2.1.280 is the first CLI that accepts the current models
/// (2.1.270 answers `400 ... version 2.1.280 or newer is required`), but the adapter's own
/// baseline for them is 2.1.283: from it `opus` and `default` are the natively 1M-token models its
/// context handling expects, and its model switch takes ~0.5 s where older CLIs take ~15 s. A
/// person's CLI below 2.1.283 runs worse under this adapter than the bundled 2.1.316, so it is
/// not used. Newer than the bundle is fine: the person's CLI is the one they run in a terminal.
const CLAUDE: UserCli = UserCli {
    harness: "registry.claude-acp",
    program: program_name(cfg!(windows)),
    variable: soyeht_ptyd::CLAUDE_CODE_EXECUTABLE,
    minimum: Version(2, 1, 283),
    #[cfg(test)]
    decided_for: "0.100.0",
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
    /// The person's program, by the path of their search path: a link that follows an update
    /// stays right where the file it points to goes away.
    Own(PathBuf),
    Bundled(Why),
}

/// The CLI to run: the person's `found` program when `version` says it is new enough. `version`
/// is asked of the file the program resolves to.
fn decide(
    minimum: Version,
    found: Option<registry::Found>,
    version: impl FnOnce(&Path) -> Option<Version>,
) -> Choice {
    let Some(found) = found else {
        return Choice::Bundled(Why::NotInstalled);
    };
    match version(&found.canonical) {
        Some(version) if version >= minimum => Choice::Own(found.entry),
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

impl Memory {
    /// Drops an `asked` that holds no answer, so the next spawn asks again at once.
    fn forget_unanswered(&mut self) {
        if self
            .asked
            .as_ref()
            .is_some_and(|asked| asked.version.is_none())
        {
            self.asked = None;
        }
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
    // The person's `PATH`, not the engine's: a program started by `env node` (an npm install)
    // finds its interpreter only there when a launch agent gave the engine a minimal one.
    command
        .arg("--version")
        .env("PATH", environment::launch_path());
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
fn settle(cli: &UserCli) -> Choice {
    let found = registry::find_executable(cli.program, environment::candidate_directories(&[]))
        // The variable is a string of the child's environment, and a path a process starts by
        // must not depend on the directory it starts in.
        .filter(|found| found.entry.to_str().is_some() && found.entry.is_absolute());
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

/// The CLI whose adapter `argv` launches: the one whose pinned package, in the registry
/// snapshot, is an argument followed by `@version`.
fn cli_of(argv: &[String]) -> Option<&'static UserCli> {
    CLIS.iter().find(|cli| {
        pinned_adapter::pinned_adapter(cli.harness).is_some_and(|adapter| {
            argv.iter().any(|argument| {
                argument
                    .strip_prefix(adapter.package.as_str())
                    .is_some_and(|rest| rest.starts_with('@'))
            })
        })
    })
}

/// An adapter process ready to be sent: its spawn, as given the person's CLI when it has one, and
/// the same spawn without it for a supervisor that refuses the variable.
pub(crate) struct Given {
    config: Spawn,
    plain: Option<Spawn>,
}

/// Gives the adapter in `config` the person's CLI to drive, when it has one. A launch that sets
/// the variable itself (a custom entry's environment) keeps its own choice; a process that is not
/// an adapter is left alone. May run `--version`, so off the Core lock.
pub(crate) fn prepare(mut config: Spawn) -> Given {
    let cli = (config.kind == ProcessKind::PipesAcp)
        .then(|| cli_of(&config.argv))
        .flatten()
        .filter(|cli| !config.env.contains_key(cli.variable));
    let mut plain = None;
    if let Some(cli) = cli
        && let Choice::Own(path) = settle(cli)
        && let Some(path) = path.to_str()
    {
        let without = config.clone();
        config.env.insert(cli.variable.into(), path.into());
        plain = Some(without);
    }
    Given { config, plain }
}

/// Whether `error` is a supervisor that does not know the variable, as one that outlived an
/// update does: it answers the spawn with this refusal and nothing else is wrong with it.
fn environment_refused(error: &Error) -> bool {
    matches!(
        error,
        Error::Supervisor(soyeht_ptyd::Error::Remote { kind, message })
            if kind == wire_kind::OPERATION_FAILED
                && message == soyeht_ptyd::ENVIRONMENT_NOT_ALLOWED
    )
}

static REFUSALS: Mutex<Streak> = Mutex::new(Streak::once());

impl Given {
    /// Asks the supervisor to start the process; if it refuses the variable, to start it
    /// without. Off the Core lock: it waits for the supervisor.
    pub(crate) fn send(self, supervisor: &Supervisor) -> Result<Response, Error> {
        self.send_with(|config| supervisor.call(Request::Spawn(config)))
    }

    fn send_with(
        self,
        mut call: impl FnMut(Spawn) -> Result<Response, Error>,
    ) -> Result<Response, Error> {
        let Some(plain) = self.plain else {
            return call(self.config);
        };
        match call(self.config) {
            Err(error) if environment_refused(&error) => {
                if streak::first_failure(&REFUSALS, &error.failure_kind()) {
                    log::failure("engine.agent_cli_refused", &error);
                }
                call(plain)
            }
            result => {
                if result.is_ok() {
                    streak::recovered(&REFUSALS);
                }
                result
            }
        }
    }
}

/// Asks for the version in the background, so the first agent pane does not wait for it. The
/// running engine calls it at start, after it has begun to ask for the launch `PATH`; a unit test
/// that opens a Core never does, so it never runs the machine's programs.
pub(crate) fn warm() {
    let started = thread::Builder::new().name("user-cli".into()).spawn(|| {
        for cli in CLIS {
            settle(cli);
            // A start on a busy host can outlast the deadline of a probe nobody waits for. The
            // first pane asks again with a deadline of its own, instead of running the bundled
            // CLI for the next 30 s because of it.
            if let Some(memory) = memory().get_mut(cli.variable) {
                memory.forget_unanswered();
            }
        }
    });
    if let Err(error) = started {
        log::failure("engine.agent_cli_warm_failed", &error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::Mutex};

    const MINIMUM: Version = Version(2, 1, 283);

    fn found() -> registry::Found {
        registry::Found {
            entry: PathBuf::from("/home/person/bin/claude"),
            canonical: PathBuf::from("/home/person/.share/claude/versions/2.1.312"),
        }
    }

    fn path() -> PathBuf {
        found().entry
    }

    #[test]
    fn a_program_that_is_new_enough_is_the_one_the_adapter_drives() {
        for version in [Version(2, 1, 283), Version(2, 1, 312), Version(3, 0, 0)] {
            assert_eq!(
                decide(MINIMUM, Some(found()), |_| Some(version)),
                Choice::Own(path())
            );
        }
    }

    #[test]
    fn the_adapter_is_given_the_path_the_person_has_and_the_version_is_asked_of_its_file() {
        let asked = Mutex::new(None);
        let choice = decide(MINIMUM, Some(found()), |file| {
            *asked.lock().unwrap() = Some(file.to_owned());
            Some(Version(2, 1, 312))
        });
        // The entry of the search path follows an update; the versioned file does not.
        assert_eq!(
            choice,
            Choice::Own(PathBuf::from("/home/person/bin/claude"))
        );
        assert_eq!(asked.into_inner().unwrap(), Some(found().canonical));
    }

    #[test]
    fn a_program_that_is_too_old_leaves_the_adapter_its_own() {
        for version in [Version(2, 1, 270), Version(2, 1, 282), Version(1, 99, 999)] {
            assert_eq!(
                decide(MINIMUM, Some(found()), |_| Some(version)),
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
            decide(MINIMUM, Some(found()), |_| None),
            Choice::Bundled(Why::VersionUnknown)
        );
    }

    #[test]
    fn the_minimum_was_decided_against_the_adapter_the_snapshot_pins() {
        for cli in CLIS {
            let pinned = pinned_adapter::pinned_adapter(cli.harness)
                .unwrap_or_else(|| panic!("{} pins no adapter package", cli.harness));
            assert_eq!(
                pinned.version, cli.decided_for,
                "the snapshot pins {} {} but the minimum of {} was decided against {}: read the \
                 new adapter's SDK and its CLI version checks, decide the minimum again and set \
                 `decided_for`",
                pinned.package, pinned.version, cli.program, cli.decided_for
            );
        }
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
    fn a_pre_release_or_build_suffix_counts_as_its_release() {
        for (output, version) in [
            ("2.1.313-beta.1 (Claude Code)", Version(2, 1, 313)),
            ("2.1.3+build", Version(2, 1, 3)),
            ("2.1.313-rc.1+build.5", Version(2, 1, 313)),
        ] {
            assert_eq!(Version::parse(output), Some(version), "{output:?}");
        }
        for refused in ["2.1-beta.1", "2.1.x-beta", "+1.2.3", "2.1.3.4-beta"] {
            assert_eq!(Version::parse(refused), None, "{refused:?}");
        }
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
    fn a_probe_that_got_no_answer_is_forgotten_and_an_answer_is_kept() {
        let (stamp, now) = (Stamp(None, 10), Instant::now());
        let mut memory = Memory::default();
        memory.version_of(&path(), stamp.clone(), now, || None);
        memory.forget_unanswered();
        let asked = Cell::new(false);
        let version = memory.version_of(&path(), stamp.clone(), now, || {
            asked.set(true);
            Some(Version(2, 1, 312))
        });
        assert_eq!((version, asked.get()), (Some(Version(2, 1, 312)), true));
        memory.forget_unanswered();
        assert_eq!(
            memory.version_of(&path(), stamp, now, || None),
            Some(Version(2, 1, 312))
        );
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

    fn pinned_claude_package() -> String {
        let pinned = pinned_adapter::pinned_adapter(CLAUDE.harness).unwrap();
        format!("{}@{}", pinned.package, pinned.version)
    }

    #[test]
    fn the_adapter_is_known_by_the_package_the_snapshot_pins() {
        let package = pinned_claude_package();
        assert!(
            package.starts_with("@") && package.contains("claude"),
            "{package}"
        );
        assert!(cli_of(&adapter(&package, ProcessKind::PipesAcp).argv).is_some());
        let name = &pinned_adapter::pinned_adapter(CLAUDE.harness)
            .unwrap()
            .package;
        for other in [
            "@agentclientprotocol/codex-acp@1.11.0".to_owned(),
            format!("{name}-extra@1.0.0"),
            name.clone(),
        ] {
            assert!(
                cli_of(&adapter(&other, ProcessKind::PipesAcp).argv).is_none(),
                "{other}"
            );
        }
    }

    #[test]
    fn a_process_that_is_not_an_adapter_or_names_its_own_cli_is_left_alone() {
        let package = pinned_claude_package();
        let terminal = adapter(&package, ProcessKind::Pty);
        let given = prepare(terminal.clone());
        assert_eq!((given.config, given.plain), (terminal, None));

        let mut recipe = adapter(&package, ProcessKind::PipesAcp);
        recipe
            .env
            .insert(CLAUDE.variable.into(), "/recipe/claude".into());
        let given = prepare(recipe.clone());
        assert_eq!((given.config, given.plain), (recipe, None));
    }

    fn instance_of(config: Spawn) -> Response {
        Response::Instance(soyeht_ptyd::wire::Instance {
            instance_id: "process".into(),
            supervisor_id: "answering".into(),
            spawn: config,
            pid: 7,
            closed: false,
            exit_code: None,
        })
    }

    fn refusal(kind: &str, message: &str) -> Error {
        Error::Supervisor(soyeht_ptyd::Error::Remote {
            kind: kind.into(),
            message: message.into(),
        })
    }

    fn given_with_cli() -> (Given, Spawn) {
        let plain = adapter(&pinned_claude_package(), ProcessKind::PipesAcp);
        let mut config = plain.clone();
        config
            .env
            .insert(CLAUDE.variable.into(), "/home/person/bin/claude".into());
        (
            Given {
                config,
                plain: Some(plain.clone()),
            },
            plain,
        )
    }

    #[test]
    fn a_supervisor_that_does_not_know_the_variable_starts_the_process_without_it() {
        let (given, plain) = given_with_cli();
        let sent = Mutex::new(Vec::new());
        let response = given
            .send_with(|config| {
                let with = config.env.contains_key(CLAUDE.variable);
                sent.lock().unwrap().push(with);
                if with {
                    Err(refusal(
                        wire_kind::OPERATION_FAILED,
                        soyeht_ptyd::ENVIRONMENT_NOT_ALLOWED,
                    ))
                } else {
                    Ok(instance_of(config))
                }
            })
            .unwrap();
        assert_eq!(*sent.lock().unwrap(), [true, false]);
        assert!(matches!(response, Response::Instance(instance) if instance.spawn == plain));
    }

    #[test]
    fn only_the_refusal_of_the_variable_is_retried_without_it() {
        let ptyd_says = [
            (wire_kind::SPAWN_REFUSED, "no such directory"),
            (
                wire_kind::SPAWN_FAILED,
                soyeht_ptyd::ENVIRONMENT_NOT_ALLOWED,
            ),
            (wire_kind::IDEMPOTENCY_CONFLICT, "instance key reused"),
            (wire_kind::OPERATION_FAILED, "invalid spawn parameters"),
        ];
        for (kind, message) in ptyd_says {
            let (given, _) = given_with_cli();
            let calls = Cell::new(0);
            let failed = given.send_with(|_| {
                calls.set(calls.get() + 1);
                Err(refusal(kind, message))
            });
            assert!(failed.is_err());
            assert_eq!(calls.get(), 1, "{kind}: {message} was sent again");
        }
    }

    #[test]
    fn a_spawn_given_no_cli_is_sent_once_as_it_is() {
        let config = adapter(&pinned_claude_package(), ProcessKind::PipesAcp);
        let calls = Cell::new(0);
        let given = Given {
            config: config.clone(),
            plain: None,
        };
        given
            .send_with(|sent| {
                calls.set(calls.get() + 1);
                assert_eq!(sent, config);
                Err(refusal(
                    wire_kind::OPERATION_FAILED,
                    soyeht_ptyd::ENVIRONMENT_NOT_ALLOWED,
                ))
            })
            .unwrap_err();
        assert_eq!(calls.get(), 1);
    }
}
