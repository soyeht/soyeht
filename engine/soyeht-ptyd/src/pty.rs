//! Pseudo-terminal primitives shared by the supervisor's panes and the engine's hidden usage
//! sessions.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Opens a pseudo terminal of `rows` × `cols`; returns `(master, slave)`, both close-on-exec.
pub fn open(rows: u16, cols: u16) -> io::Result<(File, File)> {
    let mut master = -1;
    let mut slave = -1;
    let mut size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty receives live output pointers and a valid winsize. A null
    // name and termios request the system defaults. Returned FDs are owned here.
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    } == -1
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty succeeded and returned two distinct, owned descriptors.
    let master = unsafe { File::from_raw_fd(master) };
    // SAFETY: the slave descriptor has not been wrapped or closed elsewhere.
    let slave = unsafe { File::from_raw_fd(slave) };
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        // SAFETY: both file descriptors are valid and F_SETFD takes integer flags.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((master, slave))
}

/// Makes the child lead a new session whose controlling terminal is its standard input, which
/// the caller has attached to the slave side.
pub fn control_from_stdin(command: &mut Command) {
    // SAFETY: the closure uses only async-signal-safe syscalls between fork and
    // exec. Stdio has been attached by Command before this closure executes.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY.into(), 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
