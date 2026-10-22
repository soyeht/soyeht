//! Starting a program that outlives the process that started it.
//!
//! A program a person's window starts for them (the engine, the process supervisor) must keep
//! running when the window closes and when the shell the window was started from goes away.
//!
//! - **Unix**: the child leads a session of its own (`setsid`), so it has no controlling
//!   terminal to hang up on and is not in the parent's process group.
//! - **Windows**: the child has a hidden console of its own, leads its own process group (a
//!   Ctrl+C or a closed console of the parent does not reach it) and, when the parent's job
//!   allows it, leaves that job, so a launcher that ends the job does not end the child. A job
//!   that does not allow breakaway is kept, since the child then lives as long as the job.
//!
//! Standard input is closed and standard output and error are appended to an owner-only file.
//!
//! The child also keeps no descriptor of this process above 2 (Unix): a library of a windowing
//! or graphics process may open one without close-on-exec, and it would otherwise live as long as
//! the child. On Windows `bInheritHandles` is true and the standard library passes no handle
//! list, so a handle that was created inheritable would be inherited; this process creates none.

use crate::fs::OpenOptionsPrivateExt as _;
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io,
    path::Path,
    process::{Child, Command, Stdio},
};

/// Starts `program` with `arguments`, detached from this process, with its standard output and
/// error appended to the file `log`. The folder of `log` must exist. The returned handle can be
/// dropped: dropping it does not end the child.
pub fn spawn_detached(program: &Path, arguments: &[&OsStr], log: &Path) -> io::Result<Child> {
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
    system::detach_and_spawn(&mut command)
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
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
    };

    const DETACHED: u32 = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;

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

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("soyeht-detached-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

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
    fn a_program_that_is_missing_is_an_error_of_the_start() {
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

    #[cfg(unix)]
    #[test]
    fn the_child_leads_a_session_of_its_own() {
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
