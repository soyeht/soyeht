//! Starting a process and ending it together with everything it started.
//!
//! On Unix a child leads its own process group, [`signal_group`] reaches the group and
//! [`kill_tree`] also reaches the descendants that left it. On Windows a child runs in a job
//! object and [`windows::Spawned::kill_tree`] ends the job. Both are the one place the engine
//! knows how to end "a process and what it started". [`detached::spawn_detached`] starts a
//! program that outlives the process that started it. [`end_child`] kills a `std::process::Child`
//! that still runs and reaps it within a bound, the same on every system.

pub mod detached;
mod end;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub mod windows;

use std::io;

pub use end::end_child;
#[cfg(unix)]
pub use unix::{GroupSignal, kill_tree, signal_group};

/// Asks the program that runs as process `pid` to end: a stop request on Unix (the signal that
/// closing a terminal sends, which the engine and the supervisor answer by stopping cleanly),
/// and on Windows the process is ended at once (what it started goes with it, since each runs
/// in a job). The process is not waited for.
///
/// The caller must have got `pid` from a record that is proof the process is alive and is the
/// one meant (the singleton lock, [`crate::fs::singleton_process`]), never from an old file:
/// a number can name another program once its owner has ended.
///
/// # Errors
/// [`io::ErrorKind::NotFound`] when no such process exists (it has ended already),
/// [`io::ErrorKind::InvalidInput`] for 0, 1 or this process, and the system's error otherwise,
/// such as a process of another account.
pub fn terminate(pid: u32) -> io::Result<()> {
    // 0 and 1 would reach a whole group of processes, or the init process.
    if pid <= 1 || pid == std::process::id() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    #[cfg(unix)]
    {
        unix::terminate(pid)
    }
    #[cfg(windows)]
    {
        windows::terminate(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::process::Command;

    #[test]
    fn the_reserved_ids_and_this_process_are_refused() {
        for pid in [0, 1, std::process::id()] {
            assert_eq!(
                terminate(pid).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_running_program_is_asked_to_end() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        terminate(child.id()).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_ended_is_not_found() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(terminate(pid).unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
