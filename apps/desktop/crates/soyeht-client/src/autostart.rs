//! Starting the engine when the window finds none (ADR 0102).
//!
//! A first run on Windows or Linux has no service that keeps the engine up, so the window starts
//! the engine and the process supervisor it needs from the folder of its own executable, where a
//! release ships them side by side, and then connects. They are detached (they outlive the
//! window), use the same data directory the window resolved, and are started once: a lock in
//! that directory serialises two windows, and the endpoint is checked again under it.
//!
//! The window leaves an engine alone when `SOYEHT_ENGINE_SOCKET` names one. The decisions are
//! here, behind [`Host`], so they are tested without starting a process.

use crate::profile::{SOCKET_ENV, socket_override};
use soyeht_platform::{
    fs, log,
    process::detached::{Service, create_logs_directory, spawn_service},
};
use soyeht_protocol::{
    data_directory::{RESTART_SUPERVISOR_FLAG, dev_data_directory},
    profile::DEV_PTYD_SOCKET,
};
use std::{
    any::Any,
    env::consts::EXE_SUFFIX,
    ffi::{OsStr, OsString},
    fmt, io,
    path::{Path, PathBuf},
    time::Duration,
};

const ENGINE_PROGRAM: &str = "soyeht-engine";
const SUPERVISOR_PROGRAM: &str = "soyeht-ptyd";
/// Serialises the windows that start an engine. It lives in the data directory, next to the
/// engine's own `engine.lock`.
const LOCK_NAME: &str = "desktop-start.lock";
/// How often a start looks at whether the engine answers, and how many looks it takes before it
/// gives up: 30 seconds, which a release engine on a busy host does not use.
const POLL: Duration = Duration::from_millis(100);
const POLLS: u32 = 300;

/// Whether this window starts an engine when it finds none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Autostart {
    Off,
    On(Plan),
}

/// Where the engine's data lives and where its programs are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub(crate) directory: PathBuf,
    /// The folder of this executable, when the system says where it is.
    pub(crate) programs: Option<PathBuf>,
}

impl Autostart {
    /// The decision for this process: off when the environment points at an engine, else on with
    /// the profile's data directory and the folder of this executable.
    pub fn resolve() -> Self {
        Self::from_parts(
            std::env::var_os(SOCKET_ENV),
            std::env::home_dir(),
            std::env::current_exe(),
        )
    }

    fn from_parts(
        socket_override_value: Option<OsString>,
        home: Option<PathBuf>,
        executable: io::Result<PathBuf>,
    ) -> Self {
        if socket_override(socket_override_value).is_some() {
            return Self::Off;
        }
        let Ok(directory) = dev_data_directory(home) else {
            return Self::Off;
        };
        // A system that cannot say where this program is has no folder to look in; the start
        // then names the missing program.
        let programs = match executable {
            Ok(path) => path.parent().map(Path::to_owned),
            Err(error) => {
                log::failure("desktop.executable_path_unknown", &error);
                None
            }
        };
        Self::in_directory(directory, programs)
    }

    /// A start with the data in `directory` and the programs in `programs`, when known.
    #[doc(hidden)]
    pub fn in_directory(directory: PathBuf, programs: Option<PathBuf>) -> Self {
        Self::On(Plan {
            directory,
            programs,
        })
    }
}

/// Why an engine could not be started. [`kind`](Self::kind) is what the log keeps; the message
/// is for the person and names no path.
#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    MissingProgram(&'static str),
    Directory(io::ErrorKind),
    Lock(io::ErrorKind),
    Spawn(&'static str, io::ErrorKind),
    Exited(&'static str),
    TimedOut,
    Stopped,
}

impl StartError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MissingProgram(_) => "missing_program",
            Self::Directory(_) => "directory",
            Self::Lock(_) => "lock",
            Self::Spawn(..) => "spawn",
            Self::Exited(_) => "exited",
            Self::TimedOut => "timed_out",
            Self::Stopped => "stopped",
        }
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProgram(name) => write!(
                formatter,
                "The engine could not be started: {name} is not in the folder of this program."
            ),
            Self::Exited(name) => write!(
                formatter,
                "The engine could not be started: {name} stopped right after it started."
            ),
            Self::Spawn(name, _) => write!(
                formatter,
                "The engine could not be started: the system would not run {name}."
            ),
            Self::TimedOut => {
                formatter.write_str("The engine did not answer in time. Try opening Soyeht again.")
            }
            Self::Directory(_) | Self::Lock(_) => formatter
                .write_str("The engine could not be started: its data folder cannot be used."),
            Self::Stopped => formatter.write_str("The engine start was cancelled."),
        }
    }
}

impl std::error::Error for StartError {}

impl log::Failure for StartError {
    fn failure_kind(&self) -> String {
        match self {
            Self::Directory(kind) | Self::Lock(kind) | Self::Spawn(_, kind) => {
                format!("{}.{kind:?}", self.kind())
            }
            _ => self.kind().to_owned(),
        }
    }
}

/// What a start touches outside this module, so the decisions can be tested.
pub(crate) trait Host {
    fn engine_reachable(&self) -> bool;
    fn supervisor_reachable(&self) -> bool;
    fn program_exists(&self, path: &Path) -> bool;
    /// Takes the lock that serialises starts: `None` when another start holds it. The lock
    /// ends when the returned value is dropped.
    fn lock(&self) -> io::Result<Option<Box<dyn Any>>>;
    /// Makes the folder the programs' logs go to.
    fn prepare_logs(&self) -> io::Result<()>;
    /// Starts `program`. `restart_supervisor` is the supervisor's program, which the engine is
    /// told to start again when it loses the supervisor (the window is not there to do it).
    fn start(
        &self,
        program: &Path,
        log_name: &str,
        restart_supervisor: Option<&Path>,
    ) -> io::Result<Box<dyn Started>>;
    /// Waits one poll. False when the application has stopped and the start should end.
    fn pause(&self) -> bool;
}

pub(crate) trait Started {
    fn exited(&mut self) -> bool;
}

/// Makes sure an engine answers: returns at once when one does, waits for a start that another
/// window is making, and otherwise starts the supervisor (when it is not running) and the
/// engine and waits until the engine answers.
pub(crate) fn ensure(plan: &Plan, host: &impl Host) -> Result<(), StartError> {
    if host.engine_reachable() {
        return Ok(());
    }
    let programs = plan
        .programs
        .as_deref()
        .ok_or(StartError::MissingProgram(ENGINE_PROGRAM))?;
    let engine = program_path(host, programs, ENGINE_PROGRAM)?;
    let Some(_lock) = host
        .lock()
        .map_err(|error| StartError::Lock(error.kind()))?
    else {
        // Another window is starting the engine.
        return wait_until(
            host,
            ENGINE_PROGRAM,
            || host.engine_reachable(),
            &mut || false,
        );
    };
    // The engine may have come up while this window waited for the lock.
    if host.engine_reachable() {
        return Ok(());
    }
    // A supervisor that answers needs no program next to the window.
    let supervisor = if host.supervisor_reachable() {
        None
    } else {
        Some(program_path(host, programs, SUPERVISOR_PROGRAM)?)
    };
    host.prepare_logs()
        .map_err(|error| StartError::Directory(error.kind()))?;
    if let Some(supervisor) = supervisor {
        let mut started = launch(host, &supervisor, SUPERVISOR_PROGRAM, "ptyd.log", None)?;
        wait_until(
            host,
            SUPERVISOR_PROGRAM,
            || host.supervisor_reachable(),
            &mut || started.exited(),
        )?;
    }
    // The engine keeps the supervisor it was given up, when the supervisor can be found.
    let restart = program_file(programs, SUPERVISOR_PROGRAM);
    let restart = host.program_exists(&restart).then_some(restart);
    let mut started = launch(
        host,
        &engine,
        ENGINE_PROGRAM,
        "engine.log",
        restart.as_deref(),
    )?;
    wait_until(
        host,
        ENGINE_PROGRAM,
        || host.engine_reachable(),
        &mut || started.exited(),
    )
}

fn program_file(programs: &Path, name: &str) -> PathBuf {
    programs.join(format!("{name}{EXE_SUFFIX}"))
}

fn program_path(
    host: &impl Host,
    programs: &Path,
    name: &'static str,
) -> Result<PathBuf, StartError> {
    let path = program_file(programs, name);
    if host.program_exists(&path) {
        Ok(path)
    } else {
        Err(StartError::MissingProgram(name))
    }
}

fn launch(
    host: &impl Host,
    path: &Path,
    name: &'static str,
    log_name: &str,
    restart_supervisor: Option<&Path>,
) -> Result<Box<dyn Started>, StartError> {
    host.start(path, log_name, restart_supervisor)
        .map_err(|error| StartError::Spawn(name, error.kind()))
}

/// Polls `ready` until it holds. `exited` says that `name`, the program being waited for, has
/// ended.
fn wait_until(
    host: &impl Host,
    name: &'static str,
    ready: impl Fn() -> bool,
    exited: &mut dyn FnMut() -> bool,
) -> Result<(), StartError> {
    for _ in 0..POLLS {
        if ready() {
            return Ok(());
        }
        if exited() {
            return Err(StartError::Exited(name));
        }
        if !host.pause() {
            return Err(StartError::Stopped);
        }
    }
    Err(StartError::TimedOut)
}

/// The real host: the endpoints and files of one data directory.
pub(crate) struct Local<'a> {
    pub plan: &'a Plan,
    pub socket: &'a Path,
    pub stopped: &'a dyn Fn() -> bool,
}

impl Host for Local<'_> {
    fn engine_reachable(&self) -> bool {
        crate::transport::connect(self.socket).is_ok()
    }

    fn supervisor_reachable(&self) -> bool {
        soyeht_platform::local::connect(&self.plan.directory.join(DEV_PTYD_SOCKET)).is_ok()
    }

    fn program_exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn lock(&self) -> io::Result<Option<Box<dyn Any>>> {
        match fs::lock_singleton(&self.plan.directory, LOCK_NAME) {
            Ok(lock) => Ok(Some(Box::new(lock))),
            Err(fs::LockError::Held) => Ok(None),
            Err(fs::LockError::Io(error)) => Err(error),
        }
    }

    fn prepare_logs(&self) -> io::Result<()> {
        create_logs_directory(&self.plan.directory).map(drop)
    }

    fn start(
        &self,
        program: &Path,
        log_name: &str,
        restart_supervisor: Option<&Path>,
    ) -> io::Result<Box<dyn Started>> {
        let mut extra = Vec::new();
        if let Some(supervisor) = restart_supervisor {
            extra.extend([OsStr::new(RESTART_SUPERVISOR_FLAG), supervisor.as_os_str()]);
        }
        let service = spawn_service(program, &self.plan.directory, log_name, &extra)?;
        Ok(Box::new(service))
    }

    fn pause(&self) -> bool {
        std::thread::sleep(POLL);
        !(self.stopped)()
    }
}

impl Started for Service {
    fn exited(&mut self) -> bool {
        self.has_ended()
    }
}

#[cfg(test)]
mod tests;
