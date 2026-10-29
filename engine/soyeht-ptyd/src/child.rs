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
    /// Ends the child as far as `reach` says and collects its exit. The wait is bounded because
    /// callers hold the child's lock, which the exit watcher and every other request need; a
    /// child that outlasts the limit is collected by the watcher when it does end.
    pub fn end(&mut self, reach: Reach) {
        self.kill(reach);
        match self.wait_within(REAP_LIMIT) {
            Ok(Some(_)) => {}
            Ok(None) => log::event("ptyd.child_still_running_after_kill", &[]),
            // The child could not be queried; the watcher reports it.
            Err(error) => log::failure("ptyd.child_wait_failed", &error),
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
