use std::{
    fs::File,
    io::{self, Read},
    os::fd::AsRawFd,
    time::Duration,
};

fn set_nonblocking(file: &File) -> io::Result<()> {
    // SAFETY: file owns the valid descriptor; fcntl only changes its flags.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags == -1
        // SAFETY: same owned descriptor, with the original flags preserved.
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn input_without_waiting(file: &File) -> io::Result<()> {
    set_nonblocking(file)
}

/// The read end of a terminal or pipe, in non-blocking mode.
pub(super) struct Source(File);

impl Source {
    pub(super) fn new(file: File) -> io::Result<Self> {
        set_nonblocking(&file)?;
        Ok(Self(file))
    }

    pub(super) fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }

    pub(super) fn wait_readable(&mut self, timeout: Duration) -> io::Result<()> {
        let mut descriptor = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let milliseconds = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: the file owns this descriptor throughout poll; the one-element array stays
        // valid until the call returns.
        let ready = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
        if ready < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
