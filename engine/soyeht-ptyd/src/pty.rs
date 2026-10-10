//! Pseudo-terminal sessions shared by the supervisor's panes and the engine's hidden usage
//! sessions.

use crate::{Error, Result, child::Child, io::Output};
use std::{fs::File, io, process::Command, sync::Arc};

#[cfg(unix)]
#[path = "pty/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "pty/windows.rs"]
mod system;

/// The largest rows or columns a terminal may have.
pub const MAX_DIMENSION: u16 = 1000;

/// A child running on a pseudo-terminal.
pub struct Session {
    pub child: Child,
    /// Bytes written here reach the child's terminal input. A write never blocks.
    pub input: File,
    /// What the child's terminal shows.
    pub output: Output,
    pub resizer: Resizer,
}

/// Changes the size of a session's terminal.
#[derive(Clone)]
pub struct Resizer(Arc<system::Terminal>);

impl Resizer {
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        if cols == 0 || rows == 0 || cols > MAX_DIMENSION || rows > MAX_DIMENSION {
            return Err(Error::InvalidTerminalSize);
        }
        self.0.resize(cols, rows)?;
        Ok(())
    }
}

/// Starts `command` on a new pseudo-terminal of `rows` x `cols`, leading its own session (a
/// process group on Unix, a job on Windows) whose controlling terminal it is.
///
/// The command's working directory, arguments and explicitly set environment are used; on
/// Windows, where a child has no inherited environment to clear, the explicit environment is
/// the whole of it.
pub fn spawn(command: Command, rows: u16, cols: u16) -> io::Result<Session> {
    system::spawn(command, rows, cols)
}
