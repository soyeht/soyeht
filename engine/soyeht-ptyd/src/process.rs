use crate::{
    Error, Result,
    child::Child,
    io::Output,
    pipe_guard::{self, Guard},
    pty::{self, MAX_DIMENSION, Resizer},
    wire::{ProcessKind, Spawn},
};
use soyeht_protocol::handshake::PANE_BINDING_ENV;
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

/// The variable that names the Claude Code executable the Claude adapter drives. An adapter
/// process may be given it (ADR 0137); a terminal has no use for it.
pub const CLAUDE_CODE_EXECUTABLE: &str = "CLAUDE_CODE_EXECUTABLE";

/// The one variable, with the one value, the engine gives the Codex adapter: it turns off the
/// request for a suggested next message that the adapter would send to the model, at high effort,
/// after every turn. The supervisor admits this exact pair for an adapter process and no other
/// value of the name, so the engine cannot pass an arbitrary environment through it.
pub const CODEX_PROMPT_SUGGESTIONS_OFF: (&str, &str) = ("CODEX_ACP_PROMPT_SUGGESTIONS", "false");

/// Whether the engine may set `name` to `value` for a process of `kind`. A terminal is given its
/// binding (ADR 0165); an adapter the program to drive (ADR 0137) and the fixed pair above;
/// nothing else carries one.
fn engine_may_set(name: &str, value: &str, kind: &ProcessKind) -> bool {
    matches!(
        name,
        "PATH" | "LANG" | "LC_ALL" | "TERM" | "COLORTERM" | "SHELL" | "PS1"
    ) || (name == CLAUDE_CODE_EXECUTABLE && *kind == ProcessKind::PipesAcp)
        || ((name, value) == CODEX_PROMPT_SUGGESTIONS_OFF && *kind == ProcessKind::PipesAcp)
        || (name == PANE_BINDING_ENV && *kind == ProcessKind::Pty)
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
        if !engine_may_set(name, value, &config.kind) {
            return Err(Error::EnvironmentVariableNotAllowed);
        }
        command.env(name, value);
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_of(kind: ProcessKind, name: &str) -> Spawn {
        Spawn {
            instance_key: "key".into(),
            generation: 1,
            kind,
            cwd: "/".into(),
            argv: vec!["/bin/true".into()],
            env: [(name.to_owned(), "/home/person/bin/claude".to_owned())].into(),
            cols: 80,
            rows: 24,
        }
    }

    #[test]
    fn only_an_adapter_may_be_told_which_claude_code_to_drive() {
        let claude = |kind| command(&spawn_of(kind, CLAUDE_CODE_EXECUTABLE)).map(|_| ());
        assert!(claude(ProcessKind::PipesAcp).is_ok());
        for kind in [ProcessKind::Pty, ProcessKind::PipesLsp] {
            assert!(matches!(
                claude(kind),
                Err(Error::EnvironmentVariableNotAllowed)
            ));
        }
    }

    #[test]
    fn only_an_adapter_is_given_the_fixed_pair_and_only_with_its_value() {
        let (name, value) = CODEX_PROMPT_SUGGESTIONS_OFF;
        let given = |kind, value: &str| {
            let mut config = spawn_of(kind, name);
            config.env.insert(name.into(), value.into());
            command(&config).map(|_| ())
        };
        assert!(given(ProcessKind::PipesAcp, value).is_ok());
        for refused in [
            given(ProcessKind::PipesAcp, "true"),
            given(ProcessKind::PipesAcp, ""),
            given(ProcessKind::PipesAcp, "false "),
            given(ProcessKind::Pty, value),
            given(ProcessKind::PipesLsp, value),
        ] {
            assert!(matches!(refused, Err(Error::EnvironmentVariableNotAllowed)));
        }
    }

    #[test]
    fn only_a_terminal_is_given_a_pane_binding() {
        let binding = |kind| command(&spawn_of(kind, PANE_BINDING_ENV)).map(|_| ());
        assert!(binding(ProcessKind::Pty).is_ok());
        for kind in [ProcessKind::PipesAcp, ProcessKind::PipesLsp] {
            assert!(matches!(
                binding(kind),
                Err(Error::EnvironmentVariableNotAllowed)
            ));
        }
    }

    #[test]
    fn no_other_variable_is_taken_from_the_engine() {
        for name in [
            "CLAUDE_CODE_EXECUTABLE_2",
            "CODEX_PATH",
            "HOME",
            "LD_PRELOAD",
        ] {
            assert!(matches!(
                command(&spawn_of(ProcessKind::PipesAcp, name)),
                Err(Error::EnvironmentVariableNotAllowed)
            ));
        }
        assert!(command(&spawn_of(ProcessKind::Pty, "LANG")).is_ok());
    }
}
