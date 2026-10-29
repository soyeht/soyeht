//! A process the supervisor started, and the way to end it together with what it started.

#[cfg(unix)]
#[path = "child/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "child/windows.rs"]
pub(crate) mod system;

pub use system::Child;

/// How much of a child's processes ending it reaches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reach {
    /// The child and its process group. On Unix a process a person detached from the shell with
    /// its own session stays. On Windows the job holds every process of the child, so this
    /// reaches the whole tree.
    Group,
    /// The child and everything below it, including processes that left its group.
    Tree,
}

use soyeht_platform::log;
use std::{
    io, thread,
    time::{Duration, Instant},
};

/// How long ending a child waits for it to be gone while the caller holds its lock.
const REAP_LIMIT: Duration = Duration::from_secs(2);

impl Child {
    /// Ends the child as far as `reach` says and collects its exit; true when it was collected.
    /// The wait is bounded because callers hold the child's lock, which the exit watcher and
    /// every other request need. A child that outlasts the limit is left to the exit watcher,
    /// which collects it when it does end: use this for a child that has a watcher, and
    /// [`Child::discard`] for one that has none.
    pub fn end(&mut self, reach: Reach) -> bool {
        self.kill(reach);
        match self.wait_within(REAP_LIMIT) {
            Ok(Some(_)) => true,
            Ok(None) => {
                log::event("ptyd.child_still_running_after_kill", &[]);
                false
            }
            Err(error) => {
                log::failure("ptyd.child_wait_failed", &error);
                false
            }
        }
    }

    /// Ends a child that has no exit watcher (a spawn that failed part-way) and makes sure it is
    /// collected: one that outlasts the bounded wait moves to a thread that waits for it, so no
    /// zombie stays in the long-lived supervisor.
    pub fn discard(mut self, reach: Reach) {
        if self.end(reach) {
            return;
        }
        let reaper = thread::Builder::new()
            .name("ptyd-reap".into())
            .spawn(move || {
                if let Err(error) = self.wait() {
                    log::failure("ptyd.child_wait_failed", &error);
                }
            });
        if let Err(error) = reaper {
            log::failure("ptyd.reaper_spawn_failed", &error);
        }
    }

    /// Waits for the child to end, for at most `limit`; `None` when it has not. Used after the
    /// child was killed, by callers that hold a lock the rest of the supervisor needs.
    pub fn wait_within(&mut self, limit: Duration) -> io::Result<Option<Exit>> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(exit) = self.try_wait()? {
                return Ok(Some(exit));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

/// How a process ended: its exit code, or none when a signal or the system ended it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Exit(pub Option<i32>);

impl Exit {
    pub fn code(self) -> Option<i32> {
        self.0
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    fn sleeper() -> Child {
        let inner = Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .unwrap();
        Child::new(inner)
    }

    #[test]
    fn ending_a_running_child_collects_it() {
        let mut child = sleeper();
        assert!(child.end(Reach::Group));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn ending_a_child_that_already_ended_collects_it_too() {
        let mut child = sleeper();
        assert!(child.end(Reach::Group));
        assert!(child.end(Reach::Group));
    }

    #[test]
    fn a_discarded_child_is_gone_when_discard_returns() {
        let child = sleeper();
        let pid = child.id();
        child.discard(Reach::Group);
        // SAFETY: signal 0 only checks whether the process exists; a reaped child does not.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        assert!(!alive, "the discarded child is still there");
    }
}
