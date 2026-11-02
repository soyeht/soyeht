//! Starting a program that outlives the process that started it.
//!
//! A program a person's window starts for them (the engine, the process supervisor) must keep
//! running when the window closes and when the shell the window was started from goes away.
//!
//! - **Unix**: the child leads a session of its own (`setsid`), so it has no controlling
//!   terminal to hang up on and is not in the parent's process group.
//! - **Windows**: the child has a hidden console of its own (a Ctrl+C or a closed console of the
//!   parent does not reach it) and, when the parent's job allows it, leaves that job, so a
//!   launcher that ends the job does not end the child. A job that does not allow breakaway is
//!   kept, since the child then lives as long as the job. The child does not lead a process group
//!   of its own: that flag makes the child ignore Ctrl+C, and so does everything the child starts
//!   after it, a shell in a terminal included.
//!
//! [`spawn_attached`] is the same start without the leaving: a hidden console and the same log,
//! but the child stays in the parent's job (and session), so ending that job ends the child. A
//! service host that Task Scheduler ends takes its program with it.
//!
//! Standard input is closed and standard output and error are appended to an owner-only file.
//!
//! The child also keeps no descriptor of this process above 2 (Unix): a library of a windowing
//! or graphics process may open one without close-on-exec, and it would otherwise live as long as
//! the child. On Windows `bInheritHandles` is true and the standard library passes no handle
//! list, so a handle that was created inheritable would be inherited; this process creates none.

use crate::{fs::OpenOptionsPrivateExt as _, log};
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// The folder of a data directory that holds the logs of the programs started for it.
const LOGS_DIRECTORY: &str = "logs";

/// The folder of `data_directory` that holds the logs of the programs started for it: what a
/// launch agent writes `engine.log` into, and what [`spawn_service`] writes to.
pub fn logs_directory(data_directory: &Path) -> PathBuf {
    data_directory.join(LOGS_DIRECTORY)
}

/// Makes the folder [`logs_directory`] names, owner-only, when it is missing.
pub fn create_logs_directory(data_directory: &Path) -> io::Result<PathBuf> {
    let logs = logs_directory(data_directory);
    if !logs.is_dir() {
        crate::fs::create_private_dir(&logs)?;
    }
    Ok(logs)
}

/// A program started by [`spawn_service`].
pub struct Service {
    ended: Arc<AtomicBool>,
}

impl Service {
    /// Whether the program has ended.
    pub fn has_ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }
}

/// Starts an engine program (the engine or the process supervisor) for `data_directory`, as
/// `spawn_detached` does: with `--data-dir`, then `extra`, and its output appended to
/// `logs/<log_name>`. One rule for the window and the engine, so a program started by either is
/// the same program. A thread waits for the child, so one that ends while the caller lives is
/// reaped, not left as a zombie; it ends with the child or with the caller.
pub fn spawn_service(
    program: &Path,
    data_directory: &Path,
    log_name: &str,
    extra: &[&OsStr],
) -> io::Result<Service> {
    let logs = create_logs_directory(data_directory)?;
    let mut arguments = vec![OsStr::new("--data-dir"), data_directory.as_os_str()];
    arguments.extend_from_slice(extra);
    let mut child = spawn_detached(program, &arguments, &logs.join(log_name))?;
    let ended = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ended);
    std::thread::spawn(move || {
        if let Err(error) = child.wait() {
            log::failure("process.service_wait_failed", &error);
        }
        flag.store(true, Ordering::Relaxed);
    });
    Ok(Service { ended })
}

/// Starts `program` with `arguments`, detached from this process, with its standard output and
/// error appended to the file `log`. The folder of `log` must exist. The returned handle can be
/// dropped: dropping it does not end the child.
pub fn spawn_detached(program: &Path, arguments: &[&OsStr], log: &Path) -> io::Result<Child> {
    system::detach_and_spawn(&mut command_with_log(program, arguments, log)?)
}

/// Starts `program` with `arguments` as a child of this process, with no window and its
/// standard output and error appended to the file `log`. Unlike [`spawn_detached`] the child
/// stays in this process's job (Windows) and session (Unix), so whatever ends this process's
/// job ends the child too: a service host that Task Scheduler ends takes its program with it.
pub fn spawn_attached(program: &Path, arguments: &[&OsStr], log: &Path) -> io::Result<Child> {
    system::attach_and_spawn(&mut command_with_log(program, arguments, log)?)
}

fn command_with_log(program: &Path, arguments: &[&OsStr], log: &Path) -> io::Result<Command> {
    let output = OpenOptions::new()
        .create(true)
        .append(true)
        .owner_only()
        .open_no_follow(log)?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::from(output.try_clone()?))
        .stdout(Stdio::from(output));
    Ok(command)
}

#[cfg(unix)]
mod system {
    use std::{
        io,
        os::unix::process::CommandExt as _,
        process::{Child, Command},
    };

    /// The most descriptors the fallback looks at. A limit above it leaves the descriptors
    /// above it unmarked on that path; Linux does not use the fallback unless `close_range` fails.
    const FALLBACK_LIMIT: libc::c_int = 1 << 16;

    pub(super) fn detach_and_spawn(command: &mut Command) -> io::Result<Child> {
        // SAFETY: the closure runs in the forked child before the exec and calls only
        // `setsid`, `syscall`, `sysconf` and `fcntl`, which are async-signal-safe, and touches no
        // memory of this process.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                close_on_exec_above_stderr();
                Ok(())
            });
        }
        command.spawn()
    }

    pub(super) fn attach_and_spawn(command: &mut Command) -> io::Result<Child> {
        command.spawn()
    }

    /// Marks every descriptor above 2 close-on-exec rather than closing it: the standard library's
    /// own pipe that reports a failed exec to the parent is one of them and must stay open until
    /// the exec.
    fn close_on_exec_above_stderr() {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: `close_range` with CLOEXEC only changes flags of this process's descriptors.
            let done = unsafe {
                libc::syscall(
                    libc::SYS_close_range,
                    3 as libc::c_long,
                    u32::MAX as libc::c_long,
                    libc::CLOSE_RANGE_CLOEXEC as libc::c_long,
                )
            };
            if done == 0 {
                return;
            }
        }
        // SAFETY: `sysconf` only reads a limit.
        let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        // A limit that is unlimited or cannot be read (-1) is looked at up to the fallback.
        let limit = match libc::c_int::try_from(open_max) {
            Ok(limit) if limit > 0 => limit.min(FALLBACK_LIMIT),
            _ => FALLBACK_LIMIT,
        };
        for descriptor in 3..limit {
            // SAFETY: `fcntl` on a number that is not open fails with EBADF and does nothing.
            unsafe {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags != -1 {
                    libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
        }
    }
}

#[cfg(windows)]
mod system {
    use std::{
        io,
        os::windows::process::CommandExt as _,
        process::{Child, Command},
    };
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};

    /// `CREATE_NEW_PROCESS_GROUP` is left out on purpose: it sets "ignore Ctrl+C" on the child,
    /// which every process the child starts inherits, and the child's own console already keeps
    /// the parent's Ctrl+C away.
    const DETACHED: u32 = CREATE_NO_WINDOW;

    /// A hidden console of its own and nothing else: the child stays in the job of this process.
    pub(super) fn attach_and_spawn(command: &mut Command) -> io::Result<Child> {
        command.creation_flags(CREATE_NO_WINDOW).spawn()
    }

    pub(super) fn detach_and_spawn(command: &mut Command) -> io::Result<Child> {
        // A job that does not allow breakaway refuses the flag with "access denied"; the child
        // then starts in the job as it would have without the flag.
        match command
            .creation_flags(DETACHED | CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
        {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                command.creation_flags(DETACHED).spawn()
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::testing::scratch_dir as scratch;

    #[cfg(unix)]
    fn shell(script: &str) -> (&'static Path, Vec<&OsStr>) {
        (Path::new("sh"), vec![OsStr::new("-c"), OsStr::new(script)])
    }

    #[cfg(windows)]
    fn shell(script: &str) -> (&'static Path, Vec<&OsStr>) {
        (Path::new("cmd"), vec![OsStr::new("/c"), OsStr::new(script)])
    }

    #[test]
    fn output_and_error_go_to_the_log_and_the_child_is_not_ended_by_dropping_the_handle() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        let directory = scratch("log");
        let log = directory.join("child.log");
        let (program, arguments) = shell("echo out && echo err 1>&2");
        let mut child = spawn_detached(program, &arguments, &log).unwrap();
        assert!(child.wait().unwrap().success());
        let text = fs::read_to_string(&log).unwrap();
        assert!(text.contains("out") && text.contains("err"), "{text:?}");
        // A second start appends.
        let mut child = spawn_detached(program, &arguments, &log).unwrap();
        child.wait().unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap().matches("out").count(), 2);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn an_attached_child_writes_to_the_log_and_reports_its_exit_code() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        let directory = scratch("attached");
        let log = directory.join("child.log");
        let (program, arguments) = shell("echo attached && exit 3");
        let mut child = spawn_attached(program, &arguments, &log).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(3));
        assert!(fs::read_to_string(&log).unwrap().contains("attached"));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_service_gets_the_data_directory_and_its_output_goes_to_the_logs_folder() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        use std::os::unix::fs::PermissionsExt as _;
        let directory = scratch("service");
        let program = directory.join("program");
        fs::write(&program, "#!/bin/sh\necho \"$@\"\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let data = directory.join("data");
        fs::create_dir(&data).unwrap();
        let extra = [OsStr::new("--flag"), OsStr::new("value")];
        let service = spawn_service(&program, &data, "service.log", &extra).unwrap();
        for _ in 0..200 {
            if service.has_ended() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(service.has_ended());
        let text = fs::read_to_string(logs_directory(&data).join("service.log")).unwrap();
        assert_eq!(
            text.trim(),
            format!("--data-dir {} --flag value", data.display())
        );
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_program_that_is_missing_is_an_error_of_the_start() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        let directory = scratch("missing");
        let error = spawn_detached(
            &directory.join("no-such-program"),
            &[],
            &directory.join("log"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&directory).unwrap();
    }

    /// A descriptor of this process that is not close-on-exec: the read end of a pipe.
    #[cfg(unix)]
    fn inheritable_descriptor() -> libc::c_int {
        let mut ends = [0; 2];
        // SAFETY: `ends` holds the two descriptors the call fills.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        // SAFETY: the descriptor was just opened here; the flag set is none, so it is inherited.
        assert_eq!(unsafe { libc::fcntl(ends[0], libc::F_SETFD, 0) }, 0);
        ends[0]
    }

    #[cfg(unix)]
    #[test]
    fn a_descriptor_of_this_process_does_not_reach_the_child() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        let directory = scratch("descriptors");
        let descriptor = inheritable_descriptor();
        let script = format!("test -e /dev/fd/{descriptor} && echo open || echo closed");
        let (program, arguments) = shell(&script);
        // The same script, started plainly, sees the descriptor: the test can tell the two apart.
        let plain = Command::new(program).args(&arguments).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&plain.stdout).trim(), "open");
        let log = directory.join("log");
        spawn_detached(program, &arguments, &log)
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(fs::read_to_string(&log).unwrap().trim(), "closed");
        // SAFETY: closes the descriptor this test opened.
        unsafe { libc::close(descriptor) };
        fs::remove_dir_all(&directory).unwrap();
    }

    /// The status of a process ended by Ctrl+C (`STATUS_CONTROL_C_EXIT`).
    #[cfg(windows)]
    const ENDED_BY_CTRL_C: i32 = 0xC000_013A_u32 as i32;
    /// The status of the probe when Ctrl+C did not end it.
    #[cfg(windows)]
    const SURVIVED_CTRL_C: i32 = 7;
    /// The first of the two words that tell `ctrl_c_probe` it runs as the probe, not as a test.
    #[cfg(windows)]
    const PROBE: &str = "probe-ctrl-c";
    /// The second: the probe first restores the default handling of Ctrl+C.
    #[cfg(windows)]
    const RESTORE: &str = "restore";

    /// As the test of a probe it does nothing. As the probe (a copy of this program started with
    /// [`PROBE`] among its words, the test runner taking them for filters) it sends Ctrl+C to
    /// its own console and ends with [`SURVIVED_CTRL_C`] when that did not end it.
    #[cfg(windows)]
    #[test]
    fn ctrl_c_probe() {
        let _serial = crate::testing::serialize_processes();
        use windows_sys::Win32::System::Console::{CTRL_C_EVENT, GenerateConsoleCtrlEvent};
        let words: Vec<String> = std::env::args().collect();
        if !words.iter().any(|word| word == PROBE) {
            return;
        }
        if words.iter().any(|word| word == RESTORE) {
            crate::process::windows::restore_ctrl_c().unwrap();
        }
        // SAFETY: the call only signals the processes that share this console.
        assert_ne!(unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) }, 0);
        std::thread::sleep(std::time::Duration::from_secs(5));
        std::process::exit(SURVIVED_CTRL_C);
    }

    #[cfg(windows)]
    fn probe_words(extra: &[&'static str]) -> Vec<&'static OsStr> {
        ["--exact", "process::detached::tests::ctrl_c_probe", PROBE]
            .into_iter()
            .chain(extra.iter().copied())
            .map(OsStr::new)
            .collect()
    }

    #[cfg(windows)]
    #[test]
    fn a_detached_child_ends_on_ctrl_c() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        let directory = scratch("ctrl-c");
        let program = std::env::current_exe().unwrap();
        let mut child =
            spawn_detached(&program, &probe_words(&[]), &directory.join("log")).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(ENDED_BY_CTRL_C));
        fs::remove_dir_all(&directory).unwrap();
    }

    /// What a launcher that starts a program in a process group of its own gives it, and what
    /// restoring undoes: the cause of a terminal in which Ctrl+C ended nothing.
    #[cfg(windows)]
    #[test]
    fn a_process_group_of_its_own_ignores_ctrl_c_until_it_is_restored() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        use std::os::windows::process::CommandExt as _;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
        let program = std::env::current_exe().unwrap();
        let status = |extra: &[&'static str]| {
            Command::new(&program)
                .args(probe_words(extra))
                .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .code()
        };
        assert_eq!(status(&[]), Some(SURVIVED_CTRL_C));
        assert_eq!(status(&[RESTORE]), Some(ENDED_BY_CTRL_C));
    }

    #[cfg(unix)]
    #[test]
    fn the_child_leads_a_session_of_its_own() {
        let _serial = crate::testing::serialize_processes();
        let _forks = crate::spawning();
        let directory = scratch("session");
        let (program, arguments) = shell("sleep 30");
        let mut child = spawn_detached(program, &arguments, &directory.join("log")).unwrap();
        let pid = libc::pid_t::try_from(child.id()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        // SAFETY: both calls only read the session and group of a child of this process.
        let (session, group, ours) =
            unsafe { (libc::getsid(pid), libc::getpgid(pid), libc::getsid(0)) };
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(session, pid);
        assert_eq!(group, pid);
        assert_ne!(session, ours);
        fs::remove_dir_all(&directory).unwrap();
    }
}
