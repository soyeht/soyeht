//! A private child whose control socket binds a pipe process group to ptyd.
//! EOF is not a sufficient lifetime contract for a native process's stdin.
use crate::{
    Error, Result,
    child::{Child, Reach},
    io::Output,
    process::{self, Spawned},
    wire::Spawn,
};
use serde::{Deserialize, Serialize};
use soyeht_protocol::frame::{read_json, write_json};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::{net::UnixStream, process::CommandExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
enum Reply {
    Started(u32),
    Exited(i32),
    Failed(String),
}

fn reply(control: &mut UnixStream, message: Reply) -> Result<()> {
    write_json(control, &message)?;
    Ok(())
}

/// The supervisor's end of the guardian's control socket.
pub(crate) struct Guard(UnixStream);

impl Guard {
    /// The exit code the guardian reported for the guarded process, if it reported one.
    pub(crate) fn exit_code(&mut self) -> Option<i32> {
        match read_json::<Reply>(&mut self.0).ok()?? {
            Reply::Exited(code) => Some(code),
            _ => None,
        }
    }
}

pub(crate) fn spawn(config: &Spawn, directory: &Path, id: &str) -> Result<Spawned> {
    // Validate before starting any child; this is also used by the native child.
    let _ = process::command(config)?;
    let stderr = super::open_diagnostic_log(directory, id)?;
    let (mut parent, child_socket) = UnixStream::pair()?;
    parent.set_read_timeout(Some(Duration::from_secs(5)))?;
    parent.set_write_timeout(Some(Duration::from_secs(5)))?;
    // SAFETY: duplicate into a range not used by Command's stdio setup. The
    // owned duplicate is CLOEXEC except in the specific guardian child.
    let fd = unsafe { libc::fcntl(child_socket.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
    if fd == -1 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: fcntl returned a new descriptor owned exclusively here.
    let inherited = unsafe { File::from_raw_fd(fd) };
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--pipe-guard", &fd.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr);
    // SAFETY: only async-signal-safe calls; the descriptor is kept alive through spawn.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 || libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Child::new(command.spawn()?);
    drop(inherited);
    drop(child_socket);
    let setup: Result<(u32, File, Output)> = (|| {
        write_json(&mut parent, config)?;
        let Some(reply) = read_json::<Reply>(&mut parent)? else {
            return Err(Error::MissingGuardianResponse);
        };
        let pid = match reply {
            Reply::Started(pid) => pid,
            Reply::Failed(message) => return Err(Error::GuardianFailed(message)),
            _ => return Err(Error::InvalidGuardianResponse),
        };
        let (input, output) = child_pipes(&mut child)?;
        Ok((pid, input, output))
    })();
    match setup {
        Ok((pid, input, output)) => Ok(Spawned {
            child,
            input,
            output,
            pid,
            resizer: None,
            guard: Some(Guard(parent)),
        }),
        Err(error) => {
            child.kill(Reach::Tree);
            // Reap only: the group was just terminated; an error means it is already gone.
            let _ = child.wait();
            Err(error)
        }
    }
}

/// The guardian relays the guarded process's standard input and output; take both ends.
fn child_pipes(child: &mut Child) -> Result<(File, Output)> {
    let (stdin, stdout) = child.take_pipes();
    let input = File::from(OwnedFd::from(stdin.ok_or(Error::GuardianStdinMissing)?));
    crate::io::input_without_waiting(&input)?;
    let output = File::from(OwnedFd::from(stdout.ok_or(Error::GuardianStdoutMissing)?));
    Ok((input, Output::new(output)?))
}

/// Only the private executable mode uses this inherited descriptor.
pub fn run(fd: i32) -> Result<()> {
    if fd < 64 {
        return Err(Error::InvalidGuardianDescriptor);
    }
    // SAFETY: identity-only system calls. The private mode must never address
    // a terminal's or a caller's process group when invoked outside spawn.
    if unsafe { libc::getpgrp() != libc::getpid() || libc::getsid(0) != libc::getpid() } {
        return Err(Error::GuardianNotSessionLeader);
    }
    // SAFETY: the parent gives this process ownership of the inherited socket.
    let mut control = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: prevent the harness or descendants from holding ptyd's liveness FD.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    let Some(config) = read_json::<Spawn>(&mut control)? else {
        return Ok(());
    };
    if config.argv.is_empty() {
        return Err(Error::EmptyCommand);
    }
    control.set_read_timeout(Some(Duration::from_millis(20)))?;
    control.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut child = match process::command(&config)?.spawn() {
        Ok(child) => child,
        Err(error) => {
            reply(&mut control, Reply::Failed(error.to_string()))?;
            return Ok(());
        }
    };
    if reply(&mut control, Reply::Started(child.id())).is_err() {
        kill_own_group();
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // The supervisor may already be gone; the exit is final either way.
                let _ = reply(&mut control, Reply::Exited(status.code().unwrap_or(-1)));
                kill_own_group();
            }
            // If the child cannot be waited for, nothing can be kept safe: stop the whole group.
            Err(_) => kill_own_group(),
            Ok(None) => (),
        }
        match control.read(&mut [0u8; 1]) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            // This channel carries no commands after spawn. EOF or unexpected
            // bytes close the whole group, including children ignoring stdin EOF.
            _ => kill_own_group(),
        }
    }
}

fn kill_own_group() -> ! {
    // SAFETY: the guardian is the live leader created by setsid, so this group
    // cannot have been recycled. No stored PID or other service is addressed.
    unsafe {
        libc::kill(-libc::getpgrp(), libc::SIGKILL);
        libc::_exit(1);
    }
}
