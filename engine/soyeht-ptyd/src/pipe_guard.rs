//! A private child whose control socket binds a pipe process group to ptyd.
//! EOF is not a sufficient lifetime contract for a native process's stdin.
use crate::{
    process::{self, Spawned},
    wire::Spawn,
};
use serde::{Deserialize, Serialize};
use soyeht_protocol::frame::Frame;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::{
    fs::{OpenOptionsExt, PermissionsExt},
    net::UnixStream,
    process::CommandExt,
};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
enum Reply {
    Started(u32),
    Exited(i32),
    Failed(String),
}

fn reply(control: &mut UnixStream, message: Reply) -> io::Result<()> {
    Frame::Json(serde_json::to_vec(&message)?).write_to(control)
}

pub(crate) fn spawn(config: &Spawn, directory: &Path, id: &str) -> io::Result<Spawned> {
    // Validate before starting any child; this is also used by the native child.
    let _ = process::command(config)?;
    let logs = directory.join("harness-logs");
    fs::create_dir_all(&logs)?;
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(logs.join(format!("{id}.stderr.log")))?;
    let (mut parent, child_socket) = UnixStream::pair()?;
    parent.set_read_timeout(Some(Duration::from_secs(5)))?;
    parent.set_write_timeout(Some(Duration::from_secs(5)))?;
    // SAFETY: duplicate into a range not used by Command's stdio setup. The
    // owned duplicate is CLOEXEC except in the specific guardian child.
    let fd = unsafe { libc::fcntl(child_socket.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
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
    let mut child = command.spawn()?;
    drop(inherited);
    drop(child_socket);
    let setup = (|| {
        Frame::Json(serde_json::to_vec(config)?).write_to(&mut parent)?;
        let Some(Frame::Json(bytes)) = Frame::read_from(&mut parent)? else {
            return Err(io::Error::other("missing pipe guardian response"));
        };
        let pid = match serde_json::from_slice(&bytes)? {
            Reply::Started(pid) => pid,
            Reply::Failed(message) => return Err(io::Error::other(message)),
            _ => return Err(io::Error::other("invalid pipe guardian response")),
        };
        // SAFETY: ownership moves from ChildStdin/ChildStdout into File once.
        let input =
            unsafe { File::from_raw_fd(child.stdin.take().expect("piped stdin").into_raw_fd()) };
        // SAFETY: stdout is a distinct owned descriptor.
        let output =
            unsafe { File::from_raw_fd(child.stdout.take().expect("piped stdout").into_raw_fd()) };
        process::nonblocking(&input)?;
        process::nonblocking(&output)?;
        Ok((pid, input, output))
    })();
    match setup {
        Ok((pid, input, output)) => Ok(Spawned {
            child,
            input,
            output,
            pid,
            guard: Some(parent),
        }),
        Err(error) => {
            process::terminate_group(child.id());
            let _ = child.wait();
            Err(error)
        }
    }
}

/// Only the private executable mode uses this inherited descriptor.
pub fn run(fd: i32) -> io::Result<()> {
    if fd < 64 {
        return Err(io::Error::other("invalid pipe guardian descriptor"));
    }
    // SAFETY: identity-only system calls. The private mode must never address
    // a terminal's or a caller's process group when invoked outside spawn.
    if unsafe { libc::getpgrp() != libc::getpid() || libc::getsid(0) != libc::getpid() } {
        return Err(io::Error::other("pipe guardian must own its session"));
    }
    // SAFETY: the parent gives this process ownership of the inherited socket.
    let mut control = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: prevent the harness or descendants from holding ptyd's liveness FD.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let Some(Frame::Json(bytes)) = Frame::read_from(&mut control)? else {
        return Ok(());
    };
    let config: Spawn = serde_json::from_slice(&bytes)?;
    if config.argv.is_empty() {
        return Err(io::Error::other("empty command"));
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
                let _ = reply(&mut control, Reply::Exited(status.code().unwrap_or(-1)));
                kill_own_group();
            }
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

pub(crate) fn exit_code(control: &mut UnixStream) -> Option<i32> {
    let Some(Frame::Json(bytes)) = Frame::read_from(control).ok()? else {
        return None;
    };
    match serde_json::from_slice(&bytes).ok()? {
        Reply::Exited(code) => Some(code),
        _ => None,
    }
}
