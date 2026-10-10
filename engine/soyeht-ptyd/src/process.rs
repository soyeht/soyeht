use crate::{
    Error, Result,
    child::Child,
    io::Output,
    pipe_guard::{self, Guard},
    pty::{self, MAX_DIMENSION, Resizer},
    wire::{ProcessKind, Spawn},
};
use std::fs::File;
use std::path::Path;
use std::process::Command;

pub struct Spawned {
    pub child: Child,
    pub input: File,
    pub output: Output,
    pub pid: u32,
    /// Present for a terminal, which can be resized.
    pub resizer: Option<Resizer>,
    /// Present for a pipe process, which has a guardian (Unix) or a job (Windows).
    pub guard: Option<Guard>,
}

pub fn spawn(config: &Spawn, directory: &Path, instance_id: &str) -> Result<Spawned> {
    if config.argv.is_empty()
        || config.cols == 0
        || config.rows == 0
        || config.cols > MAX_DIMENSION
        || config.rows > MAX_DIMENSION
    {
        return Err(Error::InvalidSpawnParameters);
    }
    if matches!(config.kind, ProcessKind::PipesAcp | ProcessKind::PipesLsp) {
        return pipe_guard::spawn(config, directory, instance_id);
    }
    let session = pty::spawn(command(config)?, config.rows, config.cols)?;
    Ok(Spawned {
        pid: session.child.id(),
        child: session.child,
        input: session.input,
        output: session.output,
        resizer: Some(session.resizer),
        guard: None,
    })
}

/// The values a child inherits from the supervisor. Shells need these; no engine credentials.
#[cfg(unix)]
fn inherited() -> impl Iterator<Item = &'static str> {
    [
        "HOME", "USER", "LOGNAME", "PATH", "LANG", "LC_ALL", "TMPDIR", "SHELL", "ZDOTDIR",
    ]
    .into_iter()
}

/// The variables a Windows program needs to start at all: where the system and the user's
/// folders are.
#[cfg(windows)]
pub const SYSTEM_ENVIRONMENT: &[&str] = &[
    "USERPROFILE",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "PATHEXT",
    "TEMP",
    "TMP",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "OS",
];

#[cfg(windows)]
fn inherited() -> impl Iterator<Item = &'static str> {
    SYSTEM_ENVIRONMENT
        .iter()
        .copied()
        .chain(["PATH", "LANG", "LC_ALL"])
}

pub fn command(config: &Spawn) -> Result<Command> {
    let mut command = Command::new(&config.argv[0]);
    command
        .args(&config.argv[1..])
        .current_dir(&config.cwd)
        .env_clear();
    for name in inherited() {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("TERM", "xterm-256color");
    for (name, value) in &config.env {
        if ![
            "PATH",
            "LANG",
            "LC_ALL",
            "TERM",
            "COLORTERM",
            "SHELL",
            "PS1",
        ]
        .contains(&name.as_str())
        {
            return Err(Error::EnvironmentVariableNotAllowed);
        }
        command.env(name, value);
    }
    Ok(command)
}
