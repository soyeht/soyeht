//! A hidden pseudo-terminal session for a usage source (CodexBar `ClaudeCLISession`).
//!
//! The command runs in a fresh scratch directory with a minimal environment, its screen is
//! rendered with a VT100 emulator for the source's driver, and afterwards the process group is
//! stopped, the scratch directory removed and the session transcript deleted.

use soyeht_ptyd::{
    child::{Child, Reach},
    io::Output,
    pty::Session,
};
use soyeht_usage::host::{TerminalDriver, TerminalRequest, TerminalRun, TerminalStep};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const ROWS: u16 = 50;
const COLUMNS: u16 = 160;
const POLL: Duration = Duration::from_millis(60);
const STOP_GRACE: Duration = Duration::from_secs(1);
const REMOVE_ATTEMPTS: u32 = 20;
const REMOVE_PAUSE: Duration = Duration::from_millis(50);
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
        Ok(mut session) => {
            let outcome = drive(
                &mut session.input,
                &mut session.output,
                request.timeout,
                driver,
            );
            let Session {
                mut child,
                input,
                output,
                resizer,
            } = session;
            // Close the terminal first: a process exiting with unread output waits for the
            // terminal to drain, so it would never finish while the master stays open.
            drop((input, output, resizer));
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
        remove_dir(&request.workdir)?;
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
    let _ = remove_dir(&request.workdir);
    if let Some(transcript) = &request.transcript {
        let _ = fs::remove_file(transcript);
        if let Some(parent) = transcript.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

/// Removes a scratch directory. On Windows a stopped session's processes can still hold a
/// file or the directory itself for a moment after their job reports empty, so a refusal is
/// retried for about a second. A directory that is already gone is done.
fn remove_dir(directory: &Path) -> io::Result<()> {
    let attempts = if cfg!(windows) { REMOVE_ATTEMPTS } else { 1 };
    let mut result = Ok(());
    for attempt in 0..attempts {
        if attempt > 0 {
            thread::sleep(REMOVE_PAUSE);
        }
        result = fs::remove_dir_all(directory);
        match &result {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => {}
        }
    }
    result
}

fn spawn(program: &Path, path_env: &str, request: &TerminalRequest) -> io::Result<Session> {
    let mut command = Command::new(program);
    command
        .args(&request.args)
        .current_dir(&request.workdir)
        .env_clear()
        .env("PATH", path_env)
        .env("TERM", "xterm-256color");
    for name in INHERITED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    #[cfg(windows)]
    for name in soyeht_ptyd::SYSTEM_ENVIRONMENT {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, value) in &request.env {
        command.env(name, value);
    }
    soyeht_ptyd::pty::spawn(command, ROWS, COLUMNS)
}

fn drive(
    keyboard: &mut File,
    terminal: &mut Output,
    timeout: Duration,
    driver: &mut dyn TerminalDriver,
) -> TerminalRun {
    let started = Instant::now();
    let mut parser = vt100::Parser::new(ROWS, COLUMNS, 0);
    let mut chunk = [0_u8; 8192];
    while started.elapsed() < timeout {
        loop {
            match terminal.read(&mut chunk) {
                Ok(0) => return TerminalRun::Incomplete,
                Ok(count) => {
                    let bytes = &chunk[..count];
                    if bytes.windows(CURSOR_QUERY.len()).any(|w| w == CURSOR_QUERY)
                        && keyboard.write_all(CURSOR_ANSWER).is_err()
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
                if keyboard.write_all(&bytes).is_err() {
                    return TerminalRun::Incomplete;
                }
            }
            TerminalStep::Wait => {}
        }
        thread::sleep(POLL);
    }
    TerminalRun::Incomplete
}

/// Stops the whole process group: a request to stop, a short grace, then the kill.
fn stop(child: &mut Child) {
    child.request_stop();
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        thread::sleep(POLL);
    }
    child.kill(Reach::Group);
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
            // Keys typed before the program is reading may be discarded; wait for its prompt.
            if !self.sent && screen.contains("READY") {
                self.sent = true;
                return TerminalStep::Send(b"go\r\n".to_vec());
            }
            TerminalStep::Wait
        }
    }

    /// Waits for a line, prints the panel, then stays alive for 30 seconds.
    #[cfg(unix)]
    const SCRIPT: &str = "test -f marker.json || exit 3; printf 'READY\\n'; read line; printf 'PANEL 42%%\\n'; sleep 30";
    #[cfg(windows)]
    const SCRIPT: &str = "(if not exist marker.json exit 3) & echo READY & set /p line= & echo PANEL 42% & ping -n 30 127.0.0.1 > nul";

    fn shell_name() -> &'static str {
        if cfg!(windows) { "cmd.exe" } else { "sh" }
    }

    fn shell_flag() -> &'static str {
        if cfg!(windows) { "/c" } else { "-c" }
    }

    fn shell_path() -> &'static str {
        if cfg!(windows) {
            r"C:\Windows\System32\cmd.exe"
        } else {
            "/bin/sh"
        }
    }

    fn path_env() -> &'static str {
        if cfg!(windows) {
            r"C:\Windows\System32"
        } else {
            "/usr/bin:/bin"
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
            program: shell_name().into(),
            args: vec![shell_flag().into(), SCRIPT.into()],
            env: Vec::new(),
            workdir: workdir.clone(),
            files: vec![("marker.json".into(), b"{}".to_vec())],
            transcript: Some(transcript.clone()),
            timeout: Duration::from_secs(10),
        };
        let started = Instant::now();
        let outcome = run(
            &PathBuf::from(shell_path()),
            path_env(),
            &request,
            &mut Script { sent: false },
        );
        assert!(
            matches!(&outcome, TerminalRun::Finished(screen) if screen.contains("PANEL 42%")),
            "{outcome:?}"
        );
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
