use crate::{
    Error, Result,
    wire::{ProcessKind, Spawn},
};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

pub struct Spawned {
    pub child: Child,
    pub input: File,
    pub output: File,
    pub pid: u32,
    pub guard: Option<UnixStream>,
}

pub fn spawn(config: &Spawn, directory: &Path, instance_id: &str) -> Result<Spawned> {
    if config.argv.is_empty()
        || config.cols == 0
        || config.rows == 0
        || config.cols > 1000
        || config.rows > 1000
    {
        return Err(Error::InvalidSpawnParameters);
    }
    if matches!(config.kind, ProcessKind::PipesAcp | ProcessKind::PipesLsp) {
        return crate::pipe_guard::spawn(config, directory, instance_id);
    }
    let (master, slave) = crate::pty::open(config.rows, config.cols)?;
    let mut command = command(config)?;
    command.stdin(Stdio::from(slave.try_clone()?));
    command.stdout(Stdio::from(slave.try_clone()?));
    command.stderr(Stdio::from(slave));
    crate::pty::control_from_stdin(&mut command);
    let mut child = command.spawn()?;
    let setup = nonblocking(&master).and_then(|()| master.try_clone().map_err(Error::from));
    let output = match setup {
        Ok(output) => output,
        Err(error) => {
            terminate_group(child.id());
            // Reap only: the group was just terminated; an error means it is already gone.
            let _ = child.wait();
            return Err(error);
        }
    };
    Ok(Spawned {
        pid: child.id(),
        child,
        input: master,
        output,
        guard: None,
    })
}

pub fn command(config: &Spawn) -> Result<Command> {
    let mut command = Command::new(&config.argv[0]);
    command
        .args(&config.argv[1..])
        .current_dir(&config.cwd)
        .env_clear();
    // Only these inherited values are needed by shells; no engine credentials.
    for name in [
        "HOME", "USER", "LOGNAME", "PATH", "LANG", "LC_ALL", "TMPDIR", "SHELL", "ZDOTDIR",
    ] {
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

pub fn nonblocking(file: &File) -> Result<()> {
    // SAFETY: file owns the valid descriptor; fcntl only changes its flags.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags == -1
        // SAFETY: same owned descriptor, with the original flags preserved.
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn resize(master: &File, cols: u16, rows: u16) -> Result<()> {
    if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 {
        return Err(Error::InvalidTerminalSize);
    }
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: the borrowed FD and immutable winsize remain valid during ioctl.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn terminate_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid)
        && pid > 1
    {
        // SAFETY: this is the session leader returned by our live Child,
        // never a PID loaded from a previous supervisor's registry.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

/// Ends a guarded (agent or language server) process and everything below
/// it, including children that left its process group. A terminal keeps
/// the group-only rule: a daemon the user detached from a shell stays.
pub fn terminate_tree(pid: u32) {
    // Snapshot first: once the leader dies its children are reparented and
    // no longer reachable from it.
    let escaped = crate::tree::descendants(pid);
    terminate_group(pid);
    for descendant in escaped {
        if let Ok(descendant) = i32::try_from(descendant)
            && descendant > 1
        {
            // SAFETY: the pid was read from the process table an instant ago,
            // as a descendant of the instance this supervisor spawned.
            unsafe {
                libc::kill(descendant, libc::SIGKILL);
            }
        }
    }
}
