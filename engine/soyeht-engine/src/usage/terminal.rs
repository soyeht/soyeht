//! A hidden pseudo-terminal session for a usage source (CodexBar `ClaudeCLISession`).
//!
//! The command runs in a fresh scratch directory with a minimal environment, its screen is
//! rendered with a VT100 emulator for the source's driver, and afterwards the process group is
//! stopped, the scratch directory removed and the session transcript deleted.

use super::process::harden;
use crate::streak::Streak;
use soyeht_platform::{
    fs::{LockError, create_private_dir, lock_singleton},
    log::{self, Failure},
};
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
    sync::{Mutex, PoisonError},
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

/// Failures to make or remove the scratch directory, counted across sessions so that a disk that
/// stays full, or a directory that is not ours to remove, is logged once and not on every refresh.
static SCRATCH: Mutex<Streak> = Mutex::new(Streak::once());
/// Failures to start the program, counted the same way: a program that cannot start is retried
/// on every refresh.
static SPAWN: Mutex<Streak> = Mutex::new(Streak::once());

fn scratch_failed(name: &str, error: &io::Error) {
    failed(&SCRATCH, name, error);
}

fn failed(streak: &Mutex<Streak>, name: &str, error: &io::Error) {
    let first = streak
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .fail(&error.failure_kind());
    if first {
        log::failure(name, error);
    }
}

/// Takes turns on the scratch directory. A session removes and recreates it, which would pull it
/// from under a child another session started in it: that session may be a refresh of another
/// engine on the machine (the installed app, another profile), so the turn is a lock on a
/// directory beside the scratch directory, not a lock in this process. The turn is waited for
/// within the session's own time. A path that names no directory, or a lock that cannot be made,
/// leaves the session unrun.
fn take_turn(request: &TerminalRequest) -> Option<File> {
    let name = request.workdir.file_name()?.to_string_lossy().into_owned();
    let directory = request.workdir.with_file_name(format!("{name}-lock"));
    let deadline = Instant::now() + request.timeout;
    loop {
        match lock_singleton(&directory, "session") {
            Ok(lock) => return Some(lock),
            Err(LockError::Held) if Instant::now() < deadline => thread::sleep(POLL),
            // The session reports an unavailable source; the lock's own error says nothing more.
            Err(LockError::Held | LockError::Io(..)) => return None,
        }
    }
}

pub(super) fn run(
    program: &Path,
    path_env: &str,
    request: &TerminalRequest,
    driver: &mut dyn TerminalDriver,
) -> TerminalRun {
    let Some(_turn) = take_turn(request) else {
        return TerminalRun::Incomplete;
    };
    if let Err(error) = prepare(request) {
        scratch_failed("engine.usage_scratch_failed", &error);
        return TerminalRun::Incomplete;
    }
    let outcome = match spawn(program, path_env, request) {
        Ok(mut session) => {
            SPAWN
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .succeed();
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
        // The run is incomplete, which the reading reports as unavailable.
        Err(error) => {
            failed(&SPAWN, "engine.usage_terminal_spawn_failed", &error);
            TerminalRun::Incomplete
        }
    };
    cleanup(request);
    outcome
}

/// Makes the scratch directory, owner-only, with the session's files. Called under the turn, so a
/// directory already there is what an earlier session left (or what someone else put at a
/// predictable path of a shared temporary directory); it is removed, and the new one is made
/// exclusively: a directory or link created in between is refused, not used. A failure removes
/// what was made and nothing else: the directory is not ours until it is created here.
fn prepare(request: &TerminalRequest) -> io::Result<()> {
    if fs::symlink_metadata(&request.workdir).is_ok() {
        remove_dir(&request.workdir)?;
    }
    create_private_dir(&request.workdir)?;
    fill(request).inspect_err(|_| cleanup(request))
}

fn fill(request: &TerminalRequest) -> io::Result<()> {
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
    let mut clean = true;
    let mut note = |result: io::Result<()>| {
        if let Err(error) = result {
            clean = false;
            scratch_failed("engine.usage_scratch_cleanup_failed", &error);
        }
    };
    note(remove_dir(&request.workdir));
    if let Some(transcript) = &request.transcript {
        note(ignore(
            fs::remove_file(transcript),
            &[io::ErrorKind::NotFound],
        ));
        if let Some(parent) = transcript.parent() {
            // A directory with something else in it is not ours to clear.
            note(ignore(
                fs::remove_dir(parent),
                &[io::ErrorKind::NotFound, io::ErrorKind::DirectoryNotEmpty],
            ));
        }
    }
    if clean {
        SCRATCH
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .succeed();
    }
}

/// `result` without the errors of `kinds`, which mean the thing is already as wanted.
fn ignore(result: io::Result<()>, kinds: &[io::ErrorKind]) -> io::Result<()> {
    match result {
        Err(error) if kinds.contains(&error.kind()) => Ok(()),
        other => other,
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
            // ignored: retried after a pause; the last error is returned when the attempts end.
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
    harden(&mut command);
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
                // ignored: EIO: the process closed its side of the terminal.
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
    child.end(Reach::Group);
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

    /// The script session in `workdir`, run with the scripted driver.
    fn run_script(workdir: &Path, transcript: Option<PathBuf>, timeout: u64) -> TerminalRun {
        let request = TerminalRequest {
            program: shell_name().into(),
            args: vec![shell_flag().into(), SCRIPT.into()],
            env: Vec::new(),
            workdir: workdir.to_path_buf(),
            files: vec![("marker.json".into(), b"{}".to_vec())],
            transcript,
            timeout: Duration::from_secs(timeout),
        };
        run(
            &PathBuf::from(shell_path()),
            path_env(),
            &request,
            &mut Script { sent: false },
        )
    }

    fn panel_shown(outcome: &TerminalRun) -> bool {
        matches!(outcome, TerminalRun::Finished(screen) if screen.contains("PANEL 42%"))
    }

    fn scratch_directory() -> PathBuf {
        let scratch = std::env::temp_dir().join(format!("sn-terminal-{}", Uuid::new_v4()));
        fs::create_dir_all(&scratch).unwrap();
        scratch
    }

    #[test]
    fn a_session_is_driven_and_leaves_nothing_behind() {
        let scratch = scratch_directory();
        let workdir = scratch.join("work");
        let transcript_dir = scratch.join("projects/key");
        fs::create_dir_all(&transcript_dir).unwrap();
        let transcript = transcript_dir.join("session.jsonl");
        fs::write(&transcript, b"{}").unwrap();
        let started = Instant::now();
        let outcome = run_script(&workdir, Some(transcript.clone()), 10);
        assert!(panel_shown(&outcome), "{outcome:?}");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "the session was not stopped"
        );
        assert!(!workdir.exists());
        assert!(!transcript.exists());
        assert!(!transcript_dir.exists());
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn sessions_sharing_a_scratch_directory_take_turns() {
        let scratch = scratch_directory();
        let workdir = scratch.join("work");
        let outcomes: Vec<TerminalRun> = thread::scope(|scope| {
            let runs: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| run_script(&workdir, None, 30)))
                .collect();
            runs.into_iter().map(|run| run.join().unwrap()).collect()
        });
        assert!(outcomes.iter().all(panel_shown), "{outcomes:?}");
        assert!(!workdir.exists());
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn a_lock_left_by_a_session_that_ended_blocks_nobody() {
        let scratch = scratch_directory();
        // What a crashed engine leaves: the lock directory and its file, held by no process.
        let leftover = scratch.join("work-lock");
        fs::create_dir_all(&leftover).unwrap();
        fs::write(leftover.join("session"), b"").unwrap();
        let started = Instant::now();
        let outcome = run_script(&scratch.join("work"), None, 10);
        assert!(panel_shown(&outcome), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_secs(8));
        fs::remove_dir_all(scratch).unwrap();
    }

    fn request_in(workdir: &Path) -> TerminalRequest {
        TerminalRequest {
            program: shell_name().into(),
            args: Vec::new(),
            env: Vec::new(),
            workdir: workdir.to_path_buf(),
            files: vec![("marker.json".into(), b"{}".to_vec())],
            transcript: None,
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn the_scratch_directory_is_made_owner_only_and_replaces_a_leftover() {
        let scratch = scratch_directory();
        let workdir = scratch.join("work");
        fs::create_dir_all(&workdir).unwrap();
        fs::write(workdir.join("stale"), b"left by an earlier session").unwrap();
        prepare(&request_in(&workdir)).unwrap();
        assert!(soyeht_platform::fs::dir_is_owner_only(&workdir).unwrap());
        assert!(workdir.join("marker.json").exists());
        assert!(!workdir.join("stale").exists());
        fs::remove_dir_all(scratch).unwrap();
    }

    /// What someone else could leave at the predictable path of a shared temporary directory.
    #[cfg(unix)]
    #[test]
    fn a_link_at_the_path_is_replaced_and_not_followed() {
        let scratch = scratch_directory();
        let elsewhere = scratch.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("theirs"), b"kept").unwrap();
        let workdir = scratch.join("work");
        soyeht_platform::fs::symlink(&elsewhere, &workdir).unwrap();
        prepare(&request_in(&workdir)).unwrap();
        assert!(!fs::symlink_metadata(&workdir).unwrap().is_symlink());
        assert!(workdir.join("marker.json").exists());
        assert_eq!(fs::read(elsewhere.join("theirs")).unwrap(), b"kept");
        assert!(!elsewhere.join("marker.json").exists());
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn a_session_files_that_cannot_be_written_leave_no_directory() {
        let scratch = scratch_directory();
        let workdir = scratch.join("work");
        let mut request = request_in(&workdir);
        request.files = vec![("../outside".into(), Vec::new())];
        assert!(prepare(&request).is_err());
        assert!(!workdir.exists());
        fs::remove_dir_all(scratch).unwrap();
    }
}
