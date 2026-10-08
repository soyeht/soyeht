//! A hidden pseudo-terminal session for a usage source (CodexBar `ClaudeCLISession`).
//!
//! The command runs in a fresh scratch directory with a minimal environment, its screen is
//! rendered with a VT100 emulator for the source's driver, and afterwards the process group is
//! stopped, the scratch directory removed and the session transcript deleted.

use soyeht_usage::host::{TerminalDriver, TerminalRequest, TerminalRun, TerminalStep};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::fd::AsRawFd,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const ROWS: u16 = 50;
const COLUMNS: u16 = 160;
const POLL: Duration = Duration::from_millis(60);
const STOP_GRACE: Duration = Duration::from_secs(1);
/// The screen's cursor-position query and the answer CodexBar gives it.
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CURSOR_ANSWER: &[u8] = b"\x1b[1;1R";
/// Environment a provider CLI needs to find its own login and tools; nothing else is passed.
const INHERITED: [&str; 10] = [
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "SHELL",
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    "XDG_CONFIG_HOME",
];

pub(super) fn run(
    program: &Path,
    path_env: &str,
    request: &TerminalRequest,
    driver: &mut dyn TerminalDriver,
) -> TerminalRun {
    if prepare(request).is_err() {
        cleanup(request);
        return TerminalRun::Incomplete;
    }
    let outcome = match spawn(program, path_env, request) {
        Ok((mut child, master)) => {
            let outcome = drive(&master, request.timeout, driver);
            // Close the terminal first: a process exiting with unread output waits for the
            // terminal to drain, so it would never finish while the master stays open.
            drop(master);
            stop(&mut child);
            outcome
        }
        Err(_) => TerminalRun::Incomplete,
    };
    cleanup(request);
    outcome
}

fn prepare(request: &TerminalRequest) -> io::Result<()> {
    if request.workdir.exists() {
        fs::remove_dir_all(&request.workdir)?;
    }
    fs::create_dir_all(&request.workdir)?;
    for (relative, contents) in &request.files {
        // Only plain names: nothing may land outside the scratch directory.
        let inside = Path::new(relative)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        if !inside {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let path = request.workdir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    Ok(())
}

/// Removes the scratch directory and this session's transcript, and the transcript's directory
/// when nothing else is left in it.
fn cleanup(request: &TerminalRequest) {
    let _ = fs::remove_dir_all(&request.workdir);
    if let Some(transcript) = &request.transcript {
        let _ = fs::remove_file(transcript);
        if let Some(parent) = transcript.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

fn spawn(program: &Path, path_env: &str, request: &TerminalRequest) -> io::Result<(Child, File)> {
    let (master, slave) = soyeht_ptyd::pty::open(ROWS, COLUMNS)?;
    let mut command = Command::new(program);
    command
        .args(&request.args)
        .current_dir(&request.workdir)
        .env_clear()
        .env("PATH", path_env)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    for name in INHERITED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, value) in &request.env {
        command.env(name, value);
    }
    soyeht_ptyd::pty::control_from_stdin(&mut command);
    let child = command.spawn()?;
    // SAFETY: the master descriptor is open; F_GETFL/F_SETFL only change its status flags.
    unsafe {
        let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
        if flags == -1
            || libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == -1
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((child, master))
}

fn drive(master: &File, timeout: Duration, driver: &mut dyn TerminalDriver) -> TerminalRun {
    let started = Instant::now();
    let mut parser = vt100::Parser::new(ROWS, COLUMNS, 0);
    let mut input = master;
    let mut output = master;
    let mut chunk = [0_u8; 8192];
    while started.elapsed() < timeout {
        loop {
            match input.read(&mut chunk) {
                Ok(0) => return TerminalRun::Incomplete,
                Ok(count) => {
                    let bytes = &chunk[..count];
                    if bytes.windows(CURSOR_QUERY.len()).any(|w| w == CURSOR_QUERY)
                        && output.write_all(CURSOR_ANSWER).is_err()
                    {
                        return TerminalRun::Incomplete;
                    }
                    parser.process(bytes);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                // EIO: the process closed its side of the terminal.
                Err(_) => return TerminalRun::Incomplete,
            }
        }
        let screen = parser.screen().contents();
        match driver.step(&screen, started.elapsed()) {
            TerminalStep::Done => return TerminalRun::Finished(screen),
            TerminalStep::Send(bytes) => {
                if output.write_all(&bytes).is_err() {
                    return TerminalRun::Incomplete;
                }
            }
            TerminalStep::Wait => {}
        }
        thread::sleep(POLL);
    }
    TerminalRun::Incomplete
}

/// Stops the whole process group: TERM, a short grace, then KILL.
fn stop(child: &mut Child) {
    let Ok(group) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    // SAFETY: the child called setsid, so its pid names a process group owned by this run.
    unsafe { libc::kill(-group, libc::SIGTERM) };
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        thread::sleep(POLL);
    }
    // SAFETY: as above; KILL reaches any process the CLI left in its group.
    unsafe { libc::kill(-group, libc::SIGKILL) };
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct Script {
        sent: bool,
    }

    impl TerminalDriver for Script {
        fn step(&mut self, screen: &str, _elapsed: Duration) -> TerminalStep {
            if screen.contains("PANEL 42%") {
                return TerminalStep::Done;
            }
            if !self.sent {
                self.sent = true;
                return TerminalStep::Send(b"go\n".to_vec());
            }
            TerminalStep::Wait
        }
    }

    #[test]
    fn a_session_is_driven_and_leaves_nothing_behind() {
        let scratch = std::env::temp_dir().join(format!("sn-terminal-{}", Uuid::new_v4()));
        let workdir = scratch.join("work");
        let transcript_dir = scratch.join("projects/key");
        fs::create_dir_all(&transcript_dir).unwrap();
        let transcript = transcript_dir.join("session.jsonl");
        fs::write(&transcript, b"{}").unwrap();
        let request = TerminalRequest {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "test -f marker.json || exit 3; read line; printf 'PANEL 42%%\\n'; sleep 30".into(),
            ],
            env: Vec::new(),
            workdir: workdir.clone(),
            files: vec![("marker.json".into(), b"{}".to_vec())],
            transcript: Some(transcript.clone()),
            timeout: Duration::from_secs(10),
        };
        let started = Instant::now();
        let outcome = run(
            &PathBuf::from("/bin/sh"),
            "/usr/bin:/bin",
            &request,
            &mut Script { sent: false },
        );
        assert!(matches!(outcome, TerminalRun::Finished(screen) if screen.contains("PANEL 42%")));
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "the session was not stopped"
        );
        assert!(!workdir.exists());
        assert!(!transcript.exists());
        assert!(!transcript_dir.exists());
        fs::remove_dir_all(scratch).unwrap();
    }
}
