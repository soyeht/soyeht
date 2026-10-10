//! The two ends of a child's terminal or pipes, as the supervisor reads and writes them.

use std::{
    fs::File,
    io::{self, Read},
    time::Duration,
};

#[cfg(unix)]
#[path = "io/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "io/windows.rs"]
mod system;

/// What a child writes. Reading never blocks: with nothing to read it fails with
/// [`io::ErrorKind::WouldBlock`], and [`Output::wait_readable`] waits for more.
pub struct Output(system::Source);

impl Output {
    pub(crate) fn new(file: File) -> io::Result<Self> {
        Ok(Self(system::Source::new(file)?))
    }

    /// Waits up to `timeout` for the child to write or close its end.
    pub fn wait_readable(&mut self, timeout: Duration) -> io::Result<()> {
        self.0.wait_readable(timeout)
    }
}

impl Read for Output {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

/// Makes writes to the child's input return at once instead of waiting for room: a write that
/// finds the pipe full accepts fewer bytes (or none) rather than blocking the supervisor.
pub(crate) fn input_without_waiting(file: &File) -> io::Result<()> {
    system::input_without_waiting(file)
}
