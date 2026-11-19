//! How many files a program may have open at once.
//!
//! The supervisor holds about three descriptors for every terminal and agent it runs (the master
//! side of the terminal, the readers made from it, the diagnostic log), and a person runs
//! hundreds of them. A service the system starts has a soft limit of 256 on macOS and 1024 on
//! Linux, so without this the supervisor fails to start the 80th pane and the engine reports the
//! pane as one whose process could not be started. A program that holds one descriptor per pane
//! calls [`raise_and_log`] when it starts, and the service files ask for the same number
//! ([`WANTED`]), so a start has it before any code runs.
//!
//! A raised limit is inherited by everything the program starts. A program that closes every
//! descriptor up to the limit before it runs (a runtime that starts a child, a shell) would then
//! walk tens of thousands of numbers, and a program that uses `select` breaks above 1024. So
//! whoever starts a person's program restores the limit the program was started with
//! ([`restore_in_child`]).
//!
//! Windows has no such limit to raise: a process may hold millions of handles, and the limit of
//! the C runtime's `FILE` streams (`_setmaxstdio`) does not apply to the handles Rust opens. There
//! this module reports no limit and changes nothing.

use crate::log;
use std::{io, process::Command};

/// The most descriptors a program asks for. Many times the few hundred panes a person has, and
/// small enough that a program which walks every descriptor up to its limit still finishes at
/// once. The launchd agent and the systemd unit that start the programs carry the same number.
pub const WANTED: u64 = 65_536;

/// What a program asks for when the system refuses [`WANTED`]. It is the most any macOS accepted
/// before it learned `kern.maxfilesperproc`.
#[cfg(unix)]
const FALLBACK: u64 = 10_240;

/// The two limits of a program's open files. `None` is no limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    /// The limit that is enforced, and that the program may raise up to `hard`.
    pub soft: Option<u64>,
    /// The ceiling of `soft`.
    pub hard: Option<u64>,
}

/// The limit before and after [`raise`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raised {
    pub before: Limit,
    pub now: Limit,
}

/// Raises the soft limit of open files of this process to [`WANTED`], or to the most the hard
/// limit and the system allow when that is less. A limit that is already as high is left alone:
/// this never lowers one.
///
/// # Errors
/// The error of the system when it refuses every number it was asked for.
pub fn raise() -> io::Result<Raised> {
    system::raise()
}

/// [`raise`] for a program that starts: the result is one log line, the limit it ended with
/// and the one it started with, or the kind of the failure.
pub fn raise_and_log() {
    match raise() {
        Ok(Raised { before, now }) => log::event(
            "platform.descriptor_limit",
            &[
                ("soft", &show(now.soft)),
                ("hard", &show(now.hard)),
                ("started_soft", &show(before.soft)),
            ],
        ),
        Err(error) => log::failure("platform.descriptor_limit_failed", &error),
    }
}

fn show(limit: Option<u64>) -> String {
    limit.map_or_else(|| "unlimited".to_owned(), |value| value.to_string())
}

/// Makes `command`, once started, run with the limit of open files this process started with,
/// not the one [`raise`] set. Call it on every command that starts a person's program or a tool
/// the program does not know. It does nothing before a [`raise`], and on Windows.
pub fn restore_in_child(command: &mut Command) {
    system::restore_in_child(command);
}

/// Whether `error` is the system saying the process, or the whole machine, has no descriptor
/// left to give (`EMFILE`, `ENFILE`; on Windows `ERROR_TOO_MANY_OPEN_FILES`). A person is told
/// about this one in words that name the limit; it is not a failure of the program that was asked
/// to start, and may pass when a pane closes.
pub fn is_exhausted(error: &io::Error) -> bool {
    error.raw_os_error().is_some_and(system::exhausted_code)
}

/// The limit to ask for, given the current one and the most the system accepts; `None` when the
/// current soft limit is already at least that high.
#[cfg(unix)]
fn target(soft: u64, hard: u64, system_cap: u64) -> Option<u64> {
    let wanted = WANTED.min(hard).min(system_cap);
    (wanted > soft).then_some(wanted)
}

#[cfg(unix)]
mod system {
    use super::{FALLBACK, Limit, Raised, target};
    use std::{io, os::unix::process::CommandExt as _, process::Command, sync::OnceLock};

    /// The limit this process started with, kept for the children. Set by the first raise.
    static STARTED: OnceLock<libc::rlimit> = OnceLock::new();

    fn read() -> io::Result<libc::rlimit> {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limit` is a valid, writable `rlimit` for the whole call, which only fills it in.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(limit)
    }

    fn write(limit: &libc::rlimit) -> io::Result<()> {
        // SAFETY: `limit` is a valid `rlimit` that the call only reads.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn exhausted_code(code: i32) -> bool {
        code == libc::EMFILE || code == libc::ENFILE
    }

    fn limit(soft: libc::rlim_t, hard: libc::rlim_t) -> Limit {
        let finite = |value: libc::rlim_t| (value != libc::RLIM_INFINITY).then_some(value);
        Limit {
            soft: finite(soft),
            hard: finite(hard),
        }
    }

    /// The most descriptors one process may hold, which macOS keeps apart from the limit a program
    /// sets: a soft limit above it is refused with `EINVAL`, even under an unlimited hard limit.
    #[cfg(target_os = "macos")]
    fn system_cap() -> u64 {
        let mut value: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        // SAFETY: the name is a NUL-terminated string and `value` and `size` are valid for the
        // call, which writes at most `size` bytes into `value`.
        let done = unsafe {
            libc::sysctlbyname(
                c"kern.maxfilesperproc".as_ptr(),
                (&raw mut value).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        match u64::try_from(value) {
            Ok(cap) if done == 0 && cap > 0 => cap,
            _ => FALLBACK,
        }
    }

    /// Linux has no cap below the hard limit that a program can reach with the numbers asked for.
    #[cfg(not(target_os = "macos"))]
    fn system_cap() -> u64 {
        u64::MAX
    }

    pub(super) fn raise() -> io::Result<Raised> {
        let before = read()?;
        let before_limit = limit(before.rlim_cur, before.rlim_max);
        let Some(wanted) = target(before.rlim_cur, before.rlim_max, system_cap()) else {
            return Ok(Raised {
                before: before_limit,
                now: before_limit,
            });
        };
        // The first raise keeps what the process began with, for its children.
        STARTED.get_or_init(|| libc::rlimit {
            rlim_cur: before.rlim_cur,
            rlim_max: before.rlim_max,
        });
        let mut asked = wanted;
        loop {
            let attempt = libc::rlimit {
                rlim_cur: asked,
                rlim_max: before.rlim_max,
            };
            match write(&attempt) {
                Ok(()) => {
                    return Ok(Raised {
                        before: before_limit,
                        now: limit(asked, before.rlim_max),
                    });
                }
                Err(error) => {
                    // The system has a ceiling this does not read: try the number that every system
                    // took, once.
                    if asked <= FALLBACK.max(before.rlim_cur) {
                        return Err(error);
                    }
                    asked = FALLBACK;
                }
            }
        }
    }

    pub(super) fn restore_in_child(command: &mut Command) {
        // SAFETY: the closure runs in the forked child before the exec and calls only `setrlimit`,
        // which is async-signal-safe, and reads a `OnceLock` (an atomic load); it allocates
        // nothing and takes no lock.
        unsafe {
            command.pre_exec(|| {
                let Some(original) = STARTED.get() else {
                    return Ok(());
                };
                // SAFETY: `original` is a valid `rlimit` that the call only reads.
                if libc::setrlimit(libc::RLIMIT_NOFILE, original) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(windows)]
mod system {
    use super::{Limit, Raised};
    use std::{io, process::Command};

    pub(super) fn raise() -> io::Result<Raised> {
        let none = Limit {
            soft: None,
            hard: None,
        };
        Ok(Raised {
            before: none,
            now: none,
        })
    }

    pub(super) fn restore_in_child(_command: &mut Command) {}

    /// `ERROR_TOO_MANY_OPEN_FILES`.
    pub(super) fn exhausted_code(code: i32) -> bool {
        code == 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_limit_is_raised_to_the_wish_and_never_lowered() {
        assert_eq!(target(256, u64::MAX, u64::MAX), Some(WANTED));
        assert_eq!(target(1024, u64::MAX, u64::MAX), Some(WANTED));
        assert_eq!(target(WANTED, u64::MAX, u64::MAX), None);
        assert_eq!(target(1_000_000, u64::MAX, u64::MAX), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_hard_limit_and_the_system_cap_each_bound_the_wish() {
        assert_eq!(target(256, 3000, u64::MAX), Some(3000));
        assert_eq!(target(256, u64::MAX, 10_240), Some(10_240));
        assert_eq!(target(256, 3000, 10_240), Some(3000));
        // Already at the ceiling: nothing to raise.
        assert_eq!(target(3000, 3000, u64::MAX), None);
    }

    #[cfg(unix)]
    #[test]
    fn running_out_of_descriptors_is_told_from_any_other_error() {
        for code in [libc::EMFILE, libc::ENFILE] {
            assert!(is_exhausted(&io::Error::from_raw_os_error(code)));
        }
        for code in [libc::ENOENT, libc::EACCES, libc::ENOMEM, libc::EAGAIN] {
            assert!(!is_exhausted(&io::Error::from_raw_os_error(code)));
        }
        assert!(!is_exhausted(&io::Error::from(io::ErrorKind::Other)));
    }

    #[test]
    fn a_limit_that_is_not_there_reads_as_unlimited() {
        assert_eq!(show(None), "unlimited");
        assert_eq!(show(Some(256)), "256");
    }
}
