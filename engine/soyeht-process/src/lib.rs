//! Bounded external commands.
//!
//! Every command the engine runs goes through [`run`]. It has a deadline and a
//! byte budget, and a command that misses its deadline is killed with its whole
//! process group and reported as [`Error::TimedOut`].
//!
//! A thread that holds the engine's Core lock marks itself with [`forbid`].
//! [`run`] refuses to start a command on a marked thread, so a slow command can
//! never stall the terminal path behind that lock (ADR 0079).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::{
    cell::Cell,
    fmt,
    io::{self, Read, Write},
    marker::PhantomData,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};

/// How long the pipes may stay open after the command exits or is killed.
/// A descendant that left the process group can hold them; it is abandoned.
const PIPE_GRACE: Duration = Duration::from_secs(1);
const FIRST_POLL: Duration = Duration::from_millis(1);
const LONGEST_POLL: Duration = Duration::from_millis(20);
const READ_CHUNK: usize = 8 * 1024;
/// Diagnostic text kept from standard error.
pub const STDERR_BYTES: usize = 16 * 1024;

thread_local! {
    static LOCK_HOLDER: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// The deadline and output budget of one command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}

impl Limits {
    pub const fn new(timeout: Duration, stdout_bytes: usize) -> Self {
        Self {
            timeout,
            stdout_bytes,
            stderr_bytes: STDERR_BYTES,
        }
    }
}

#[derive(Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Standard error as trimmed text for an error message.
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_owned()
    }
}

#[derive(Debug)]
pub enum Error {
    Spawn {
        program: String,
        source: io::Error,
    },
    Wait {
        program: String,
        source: io::Error,
    },
    Read {
        program: String,
        source: io::Error,
    },
    TimedOut {
        program: String,
        after: Duration,
    },
    OutputTooLarge {
        program: String,
    },
    /// The input could not be delivered to the command's standard input.
    Write {
        program: String,
        source: io::Error,
    },
    UnderLock {
        program: String,
        holder: &'static str,
    },
}

impl Error {
    pub fn timed_out(&self) -> bool {
        matches!(self, Self::TimedOut { .. })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { program, source } => {
                write!(formatter, "{program} did not start: {source}")
            }
            Self::Wait { program, source } => {
                write!(formatter, "{program} could not be awaited: {source}")
            }
            Self::Read { program, source } => {
                write!(formatter, "{program} output could not be read: {source}")
            }
            Self::TimedOut { program, after } => write!(
                formatter,
                "{program} timed out after {} ms and was killed",
                after.as_millis()
            ),
            Self::OutputTooLarge { program } => {
                write!(formatter, "{program} output exceeds its byte budget")
            }
            Self::Write { program, source } => {
                write!(formatter, "{program} input could not be written: {source}")
            }
            Self::UnderLock { program, holder } => write!(
                formatter,
                "{program} was refused because the {holder} lock is held; blocking work runs off that lock"
            ),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Marks the current thread as holding `holder` until the guard drops.
pub fn forbid(holder: &'static str) -> Forbidden {
    Forbidden {
        previous: LOCK_HOLDER.replace(Some(holder)),
        _thread_bound: PhantomData,
    }
}

/// The lock the current thread holds, if it is marked.
pub fn forbidden() -> Option<&'static str> {
    LOCK_HOLDER.get()
}

/// Restores the previous mark when dropped. It stays on its thread.
pub struct Forbidden {
    previous: Option<&'static str>,
    _thread_bound: PhantomData<*const ()>,
}

impl Drop for Forbidden {
    fn drop(&mut self) {
        LOCK_HOLDER.set(self.previous);
    }
}

/// Runs `command` with no standard input, within `limits`.
pub fn run(command: Command, limits: Limits) -> Result<Output> {
    execute(command, None, limits)
}

/// Runs `command` with `input` on its standard input, within `limits`.
///
/// The input is written from its own thread and then closed, so a command that
/// answers before it reads everything cannot stall the caller.
pub fn run_with_input(command: Command, input: Vec<u8>, limits: Limits) -> Result<Output> {
    execute(command, Some(input), limits)
}

fn execute(mut command: Command, input: Option<Vec<u8>>, limits: Limits) -> Result<Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    if let Some(holder) = forbidden() {
        return Err(Error::UnderLock { program, holder });
    }
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|source| Error::Spawn {
        program: program.clone(),
        source,
    })?;
    let written = input.map(|input| write_in_background(&mut child, input));
    let pipes = Pipes::start(&mut child, limits);
    let exited = match wait_until(&mut child, Instant::now() + limits.timeout) {
        Ok(exited) => exited,
        Err(source) => {
            kill_group(&mut child);
            return Err(Error::Wait { program, source });
        }
    };
    if exited.is_none() {
        kill_group(&mut child);
    }
    let collected = pipes
        .collect(Instant::now() + PIPE_GRACE)
        .map_err(|source| Error::Read {
            program: program.clone(),
            source,
        })?;
    let timed_out = Error::TimedOut {
        program: program.clone(),
        after: limits.timeout,
    };
    let (Some(status), Some(collected)) = (exited, collected) else {
        return Err(timed_out);
    };
    let (Some(stdout), Some(stderr)) = (collected.stdout, collected.stderr) else {
        return Err(Error::OutputTooLarge { program });
    };
    if let Some(written) = written {
        input_delivered(written, Instant::now() + PIPE_GRACE)
            .map_err(|source| Error::Write { program, source })?;
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Starts the thread that writes `input` to the child and closes its standard input.
fn write_in_background(child: &mut Child, input: Vec<u8>) -> thread::JoinHandle<io::Result<()>> {
    let stdin = child.stdin.take();
    thread::spawn(move || match stdin {
        Some(mut stdin) => stdin.write_all(&input),
        None => Err(io::Error::from(io::ErrorKind::NotConnected)),
    })
}

/// Whether the input reached the command. A command that exited without reading it
/// all closes the pipe, which its exit status reports instead. A writer still blocked
/// at `deadline` is held by a process that left the group, and is abandoned.
fn input_delivered(
    writer: thread::JoinHandle<io::Result<()>>,
    deadline: Instant,
) -> io::Result<()> {
    while !writer.is_finished() {
        if Instant::now() >= deadline {
            return Ok(());
        }
        thread::sleep(FIRST_POLL);
    }
    match writer.join() {
        Ok(Err(error)) if error.kind() != io::ErrorKind::BrokenPipe => Err(error),
        Ok(_) => Ok(()),
        Err(_) => Err(io::Error::other("input writer panicked")),
    }
}

fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    let mut pause = FIRST_POLL;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(LONGEST_POLL);
    }
}

fn kill_group(child: &mut Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: killpg only sends a signal. The child is not reaped yet, so
        // its pid still names the process group it leads (process_group(0)).
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
    // Best effort: the caller reports the timeout, whatever happens to the child here.
    // kill fails only when the child already exited, which is the goal.
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

/// Captured output; `None` marks a stream that went over its budget.
struct Collected {
    stdout: Option<Vec<u8>>,
    stderr: Option<Vec<u8>>,
}

struct Pipes {
    receiver: Receiver<(Stream, io::Result<Option<Vec<u8>>>)>,
    open: usize,
}

impl Pipes {
    fn start(child: &mut Child, limits: Limits) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut open = 0;
        if let Some(stdout) = child.stdout.take() {
            read_in_background(stdout, Stream::Stdout, limits.stdout_bytes, &sender);
            open += 1;
        }
        if let Some(stderr) = child.stderr.take() {
            read_in_background(stderr, Stream::Stderr, limits.stderr_bytes, &sender);
            open += 1;
        }
        Self { receiver, open }
    }

    /// `Ok(None)` means a reader did not finish by `deadline`; a reader that
    /// failed is an error.
    fn collect(self, deadline: Instant) -> io::Result<Option<Collected>> {
        let mut collected = Collected {
            stdout: Some(Vec::new()),
            stderr: Some(Vec::new()),
        };
        for _ in 0..self.open {
            let wait = deadline.saturating_duration_since(Instant::now());
            let (stream, bytes) = match self.receiver.recv_timeout(wait) {
                Ok(message) => message,
                // Late or gone: the caller reports the deadline.
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    return Ok(None);
                }
            };
            let bytes = bytes?;
            match stream {
                Stream::Stdout => collected.stdout = bytes,
                Stream::Stderr => collected.stderr = bytes,
            }
        }
        Ok(Some(collected))
    }
}

fn read_in_background<R: Read + Send + 'static>(
    reader: R,
    stream: Stream,
    limit: usize,
    sender: &Sender<(Stream, io::Result<Option<Vec<u8>>>)>,
) {
    let sender = sender.clone();
    thread::spawn(move || {
        // The receiver is gone once the deadline passed; nobody is left to read this result.
        let _ = sender.send((stream, read_bounded(reader, limit)));
    });
}

/// Reads to the end. Bytes past `limit` are drained so the writer never blocks,
/// and the result is `None`.
fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut kept = Vec::new();
    let mut over = false;
    let mut chunk = [0_u8; READ_CHUNK];
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => return Ok((!over).then_some(kept)),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if kept.len().saturating_add(count) > limit {
            over = true;
            kept = Vec::new();
        } else if !over {
            kept.extend_from_slice(&chunk[..count]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        command
    }

    fn scratch(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("soyeht-process-{label}-{}", std::process::id()))
    }

    fn alive(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn a_command_returns_its_status_and_both_streams() {
        let output = run(
            shell("printf out; printf err >&2; exit 3"),
            Limits::new(Duration::from_secs(10), 64),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(!output.success());
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr_text(), "err");
    }

    #[test]
    fn a_hung_command_is_killed_with_its_descendants_and_reported() {
        let pidfile = scratch("hung");
        let _ = fs::remove_file(&pidfile);
        let started = Instant::now();
        let error = run(
            shell(&format!(
                "sleep 30 & echo $! > '{}'; wait",
                pidfile.display()
            )),
            Limits::new(Duration::from_millis(300), 64),
        )
        .unwrap_err();
        let elapsed = started.elapsed();
        assert!(error.timed_out(), "{error}");
        assert!(
            error.to_string().contains("timed out after 300 ms"),
            "{error}"
        );
        // The deadline plus the pipe grace bounds the call, far below the
        // 30 s the descendant would have slept.
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
        let descendant: libc::pid_t = fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(descendant) {
            assert!(
                Instant::now() < deadline,
                "descendant {descendant} survived"
            );
            thread::sleep(Duration::from_millis(10));
        }
        fs::remove_file(pidfile).unwrap();
    }

    #[test]
    fn output_over_budget_is_drained_and_reported() {
        let error = run(
            shell("head -c 100000 /dev/zero"),
            Limits::new(Duration::from_secs(10), 1024),
        )
        .unwrap_err();
        assert!(matches!(error, Error::OutputTooLarge { .. }), "{error}");
    }

    #[test]
    fn input_reaches_the_command_and_a_command_that_ignores_it_is_not_stalled() {
        let echoed = run_with_input(
            Command::new("/bin/cat"),
            b"config".to_vec(),
            Limits::new(Duration::from_secs(10), 64),
        )
        .unwrap();
        assert_eq!(echoed.stdout, b"config");
        // Larger than a pipe buffer, and never read: the writer must not hold the call.
        let started = Instant::now();
        let ignored = run_with_input(
            shell("exit 0"),
            vec![b'x'; 1 << 20],
            Limits::new(Duration::from_secs(10), 64),
        )
        .unwrap();
        assert!(ignored.success());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_command_that_never_reads_its_input_times_out_instead_of_hanging() {
        let error = run_with_input(
            shell("sleep 30"),
            vec![b'x'; 1 << 20],
            Limits::new(Duration::from_millis(300), 64),
        )
        .unwrap_err();
        assert!(error.timed_out(), "{error}");
    }

    #[test]
    fn a_thread_holding_the_core_lock_cannot_start_a_command() {
        {
            let _held = forbid("Core");
            assert_eq!(forbidden(), Some("Core"));
            let error = run(shell("true"), Limits::new(Duration::from_secs(10), 64)).unwrap_err();
            assert!(
                matches!(error, Error::UnderLock { holder: "Core", .. }),
                "{error}"
            );
        }
        assert_eq!(forbidden(), None);
        assert!(
            run(shell("true"), Limits::new(Duration::from_secs(10), 64))
                .unwrap()
                .success()
        );
    }
}
