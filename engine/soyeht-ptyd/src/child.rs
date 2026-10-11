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

use std::{
    io, thread,
    time::{Duration, Instant},
};

impl Child {
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
