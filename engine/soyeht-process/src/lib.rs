//! Bounded external commands.
//!
//! Every command the engine runs goes through [`run`]. It has a deadline and a
//! byte budget, and a command that misses its deadline is killed with everything it started
//! (its process group on Unix, its job object on Windows) and reported as
//! [`Error::TimedOut`].
//!
//! Every start in the engine process goes through one lock, [`starting`], so that no command
//! inherits the pipes of another that is starting at the same moment.
//!
//! A thread that holds the engine's Core lock marks itself with [`forbid`].
//! [`run`] refuses to start a command on a marked thread, so a slow command can
//! never stall the terminal path behind that lock (ADR 0079).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod git;

#[cfg(unix)]
#[path = "system/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "system/windows.rs"]
mod system;

use std::{
    cell::Cell,
    fmt,
    fs::File,
    io::{self, Read, Write},
    marker::PhantomData,
    process::{Command, ExitStatus},
    sync::{
        Mutex, MutexGuard, PoisonError, TryLockError,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

pub use git::{GIT_UNAVAILABLE_MESSAGE, require_git, require_git_with, run_git, run_git_with};
use system::Running;

/// How long the pipes may stay open after the command exits or is killed.
/// A descendant that left the process group can hold them; it is abandoned.
/// [`starting`] keeps a concurrent start from holding them too.
const PIPE_GRACE: Duration = Duration::from_secs(1);
const FIRST_POLL: Duration = Duration::from_millis(1);
const LONGEST_POLL: Duration = Duration::from_millis(20);
const READ_CHUNK: usize = 8 * 1024;
/// Diagnostic text kept from standard error.
pub const STDERR_BYTES: usize = 16 * 1024;

/// Held while a process starts. A start creates the child's pipes, and a process another
/// thread starts at the same moment can inherit their write ends:
///
/// - On Unix a fork copies every open descriptor, `CLOEXEC` ones included, and drops them only
///   at `exec`. Until the second program image has loaded, which on a loaded macOS host takes
///   seconds because the system assesses the first run of a new binary, it holds the first
///   command's write ends.
/// - macOS has no atomic `pipe2`: `pipe()` and then `fcntl(FD_CLOEXEC)` leave a window in
///   which a concurrent fork inherits the new descriptors for good.
///
/// Either way the first command's output stays open long after it exited, and its collector
/// waits for an end that does not come. One start at a time leaves no overlap. [`run`] holds
/// the lock for its own starts, and every other start in the process uses [`starting`].
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

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
    /// `git` was not started: it would open the system installer, or it is not installed.
    GitUnavailable,
}

/// A failure to run a command logs as a bounded label, never as message text: the text names the
/// program and carries paths.
impl soyeht_platform::log::Failure for Error {
    fn failure_kind(&self) -> String {
        match self {
            Self::Spawn { source, .. } => format!("spawn:{:?}", source.kind()),
            Self::Wait { source, .. } => format!("wait:{:?}", source.kind()),
            Self::Read { source, .. } => format!("read:{:?}", source.kind()),
            Self::Write { source, .. } => format!("write:{:?}", source.kind()),
            Self::TimedOut { .. } => "timed_out".to_owned(),
            Self::OutputTooLarge { .. } => "output_too_large".to_owned(),
            Self::UnderLock { .. } => "under_lock".to_owned(),
            Self::GitUnavailable => "git_unavailable".to_owned(),
        }
    }
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
            Self::GitUnavailable => formatter.write_str(GIT_UNAVAILABLE_MESSAGE),
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

/// Runs `start`, which starts the process `program`, with no other start in progress in this
/// process. A thread that holds the Core lock is refused, as [`run`] refuses it: it would make
/// every other start wait behind that lock.
///
/// Hold only the start itself: a handshake with the new process, or anything else that waits,
/// belongs after it, since every other start queues behind this one.
pub fn starting<T>(program: &str, start: impl FnOnce() -> T) -> Result<T> {
    if let Some(holder) = forbidden() {
        return Err(Error::UnderLock {
            program: program.to_owned(),
            holder,
        });
    }
    let _starting = lock_starts();
    Ok(start())
}

fn lock_starts() -> MutexGuard<'static, ()> {
    // The lock guards no data, so a start that panicked left nothing to repair.
    SPAWN_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// [`starting`], unless another start keeps the lock until `deadline`: the time a command
/// queues behind other starts counts against its own timeout.
fn starting_before<T>(deadline: Instant, start: impl FnOnce() -> T) -> Option<T> {
    let mut pause = FIRST_POLL;
    loop {
        match SPAWN_LOCK.try_lock() {
            Ok(_starting) => return Some(start()),
            Err(TryLockError::Poisoned(poisoned)) => {
                let _starting = poisoned.into_inner();
                return Some(start());
            }
            Err(TryLockError::WouldBlock) => {}
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(LONGEST_POLL);
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

/// The pipes of a running command. The input exists only when the caller has input to send.
pub(crate) struct Streams {
    pub(crate) input: Option<File>,
    pub(crate) output: Option<File>,
    pub(crate) error: Option<File>,
}

fn execute(command: Command, input: Option<Vec<u8>>, limits: Limits) -> Result<Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    if let Some(holder) = forbidden() {
        return Err(Error::UnderLock { program, holder });
    }
    let deadline = Instant::now() + limits.timeout;
    let timed_out = || Error::TimedOut {
        program: program.clone(),
        after: limits.timeout,
    };
    let mut running = starting_before(deadline, || Running::spawn(command, input.is_some()))
        .ok_or_else(timed_out)?
        .map_err(|source| Error::Spawn {
            program: program.clone(),
            source,
        })?;
    let streams = running.take_streams();
    let written = input.map(|input| write_in_background(streams.input, input));
    let pipes = Pipes::start((streams.output, streams.error), limits);
    let exited = match wait_until(&mut running, deadline) {
        Ok(exited) => exited,
        Err(source) => {
            running.kill();
            return Err(Error::Wait { program, source });
        }
    };
    if exited.is_none() {
        running.kill();
    } else {
        running.release();
    }
    let collected = pipes
        .collect(Instant::now() + PIPE_GRACE)
        .map_err(|source| Error::Read {
            program: program.clone(),
            source,
        })?;
    let (Some(status), Some(collected)) = (exited, collected) else {
        return Err(timed_out());
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

/// Starts the thread that writes `input` to the command and closes its standard input.
fn write_in_background(stdin: Option<File>, input: Vec<u8>) -> thread::JoinHandle<io::Result<()>> {
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
        // ignored: the panic becomes the error the caller gets.
        Err(_) => Err(io::Error::other("input writer panicked")),
    }
}

fn wait_until(child: &mut Running, deadline: Instant) -> io::Result<Option<ExitStatus>> {
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
    fn start((stdout, stderr): (Option<File>, Option<File>), limits: Limits) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut open = 0;
        if let Some(stdout) = stdout {
            read_in_background(stdout, Stream::Stdout, limits.stdout_bytes, &sender);
            open += 1;
        }
        if let Some(stderr) = stderr {
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
        // ignored: the receiver is gone once the deadline passed; nobody is left to read this result.
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
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    /// The test executable doubles as the helper program the tests run, so they behave the
    /// same on every system: `HELPER_MODE` selects what the helper does (see [`helper`]).
    const HELPER_MODE: &str = "SOYEHT_PROCESS_TEST_HELPER";

    fn helper_command(mode: &str) -> Command {
        let mut command = Command::new(env::current_exe().unwrap());
        command
            .args(["--exact", "tests::helper", "--nocapture"])
            .env(HELPER_MODE, mode);
        command
    }

    /// Does nothing in a normal test run. As a helper it plays the program that a test runs.
    #[test]
    fn helper() {
        let Ok(mode) = env::var(HELPER_MODE) else {
            return;
        };
        let (mode, argument) = mode.split_once(':').unwrap_or((&mode, ""));
        match mode {
            "emit" => {
                print!("out");
                eprint!("err");
                std::process::exit(3);
            }
            "flood" => {
                print!("{}", "x".repeat(100_000));
            }
            // Starts a sleeping descendant, records its pid, and waits for it.
            "hang" => {
                let mut descendant = helper_command("sleep").spawn().unwrap();
                fs::write(argument, descendant.id().to_string()).unwrap();
                descendant.wait().unwrap();
            }
            "sleep" => thread::sleep(Duration::from_secs(30)),
            // Leaves a sleeping background process behind and ends at once.
            "detach" => {
                let descendant = helper_command("sleep").spawn().unwrap();
                fs::write(argument, descendant.id().to_string()).unwrap();
                // Only its pid matters: the test ends it.
                std::mem::forget(descendant);
            }
            // Writes what arrives on standard input back to standard output.
            "echo" => {
                let mut input = Vec::new();
                io::stdin().read_to_end(&mut input).unwrap();
                io::stdout().write_all(&input).unwrap();
            }
            _ => {}
        }
    }

    fn scratch(label: &str) -> PathBuf {
        env::temp_dir().join(format!("soyeht-process-{label}-{}", std::process::id()))
    }

    #[cfg(unix)]
    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid.try_into().unwrap(), 0) == 0 }
    }

    #[cfg(windows)]
    fn alive(pid: u32) -> bool {
        let listing = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&listing.stdout).contains(&format!("\"{pid}\""))
    }

    #[cfg(unix)]
    fn end_process(pid: u32) {
        // SAFETY: the pid is a sleeping helper this test started and still owns.
        unsafe { libc::kill(pid.try_into().unwrap(), libc::SIGKILL) };
    }

    #[cfg(windows)]
    fn end_process(pid: u32) {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .unwrap();
    }

    fn read_pid(file: &Path) -> u32 {
        fs::read_to_string(file).unwrap().trim().parse().unwrap()
    }

    #[test]
    fn a_command_returns_its_status_and_both_streams() {
        let output = run(
            helper_command("emit"),
            Limits::new(Duration::from_secs(30), 8192),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(!output.success());
        // The test harness prints its own lines around the helper's.
        assert!(String::from_utf8_lossy(&output.stdout).contains("out"));
        assert!(output.stderr_text().contains("err"));
    }

    #[test]
    fn a_hung_command_is_killed_with_its_descendants_and_reported() {
        let pidfile = scratch("hung");
        let _ = fs::remove_file(&pidfile);
        let started = Instant::now();
        // Long enough for a loaded host to start the helper and its descendant.
        let error = run(
            helper_command(&format!("hang:{}", pidfile.display())),
            Limits::new(Duration::from_secs(5), 8192),
        )
        .unwrap_err();
        let elapsed = started.elapsed();
        assert!(error.timed_out(), "{error}");
        assert!(
            error.to_string().contains("timed out after 5000 ms"),
            "{error}"
        );
        // The deadline plus the pipe grace bounds the call, far below the
        // 30 s the descendant would have slept.
        assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
        let descendant = read_pid(&pidfile);
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
    fn a_command_that_finishes_leaves_its_background_process_running() {
        let pidfile = scratch("detached");
        let _ = fs::remove_file(&pidfile);
        let mut running = starting("helper", || {
            Running::spawn(
                helper_command(&format!("detach:{}", pidfile.display())),
                false,
            )
        })
        .unwrap()
        .unwrap();
        // The pipes stay open so the helper never fails writing to them.
        let _streams = running.take_streams();
        let deadline = Instant::now() + Duration::from_secs(30);
        while running.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the helper did not finish");
            thread::sleep(Duration::from_millis(10));
        }
        running.release();
        drop(running);
        // Dropping the last handle would end the job's processes if it were not released.
        thread::sleep(Duration::from_millis(500));
        let descendant = read_pid(&pidfile);
        let survived = alive(descendant);
        end_process(descendant);
        fs::remove_file(pidfile).unwrap();
        assert!(
            survived,
            "the background process was ended with the command"
        );
    }

    #[test]
    fn output_over_budget_is_drained_and_reported() {
        let error = run(
            helper_command("flood"),
            Limits::new(Duration::from_secs(30), 1024),
        )
        .unwrap_err();
        assert!(matches!(error, Error::OutputTooLarge { .. }), "{error}");
    }

    #[test]
    fn input_reaches_the_command_and_a_command_that_ignores_it_is_not_stalled() {
        let echoed = run_with_input(
            helper_command("echo"),
            b"config".to_vec(),
            Limits::new(Duration::from_secs(30), 8192),
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&echoed.stdout).contains("config"));
        // Larger than a pipe buffer, and never read: the writer must not hold the call.
        let started = Instant::now();
        let ignored = run_with_input(
            helper_command("quiet"),
            vec![b'x'; 1 << 20],
            Limits::new(Duration::from_secs(30), 8192),
        )
        .unwrap();
        assert!(ignored.success());
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_command_that_never_reads_its_input_times_out_instead_of_hanging() {
        let error = run_with_input(
            helper_command("sleep"),
            vec![b'x'; 1 << 20],
            Limits::new(Duration::from_secs(2), 8192),
        )
        .unwrap_err();
        assert!(error.timed_out(), "{error}");
    }

    #[test]
    fn a_thread_holding_the_core_lock_cannot_start_a_command() {
        {
            let _held = forbid("Core");
            assert_eq!(forbidden(), Some("Core"));
            let error = run(
                helper_command("quiet"),
                Limits::new(Duration::from_secs(30), 8192),
            )
            .unwrap_err();
            assert!(
                matches!(error, Error::UnderLock { holder: "Core", .. }),
                "{error}"
            );
        }
        assert_eq!(forbidden(), None);
        assert!(
            run(
                helper_command("quiet"),
                Limits::new(Duration::from_secs(30), 8192)
            )
            .unwrap()
            .success()
        );
    }

    #[test]
    fn a_thread_holding_the_core_lock_cannot_queue_a_start() {
        let _held = forbid("Core");
        let error = starting("helper", || ()).unwrap_err();
        assert!(
            matches!(error, Error::UnderLock { holder: "Core", .. }),
            "{error}"
        );
    }

    #[test]
    fn commands_started_together_all_finish_and_none_reports_a_timeout() {
        let runners: Vec<_> = (0..8)
            .map(|_| {
                thread::spawn(|| {
                    (0..3)
                        .map(|_| {
                            run(
                                helper_command("emit"),
                                Limits::new(Duration::from_secs(60), 8192),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for runner in runners {
            for result in runner.join().unwrap() {
                let output = result.unwrap();
                assert_eq!(output.status.code(), Some(3));
                assert!(String::from_utf8_lossy(&output.stdout).contains("out"));
                assert!(output.stderr_text().contains("err"));
            }
        }
    }

    #[test]
    fn the_time_a_command_queues_behind_other_starts_counts_against_its_timeout() {
        let (held, holding) = mpsc::channel();
        let blocker = thread::spawn(move || {
            starting("helper", || {
                held.send(()).unwrap();
                thread::sleep(Duration::from_secs(3));
            })
            .unwrap();
        });
        holding.recv().unwrap();
        let started = Instant::now();
        let error = run(
            helper_command("quiet"),
            Limits::new(Duration::from_millis(200), 8192),
        )
        .unwrap_err();
        assert!(error.timed_out(), "{error}");
        // It gave up at its own deadline instead of waiting out the start in front of it.
        assert!(started.elapsed() < Duration::from_millis(2500), "{error}");
        blocker.join().unwrap();
    }
}
