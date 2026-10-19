use super::{Resizer, Session};
use crate::{
    child::{Child, Reach},
    io::Output,
};
use std::{
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::Arc,
};

/// The master side of the terminal.
pub(super) struct Terminal(File);

impl Terminal {
    pub(super) fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: the borrowed FD and immutable winsize remain valid during ioctl.
        if unsafe { libc::ioctl(self.0.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Opens a pseudo terminal of `rows` x `cols`; returns `(master, slave)`, both close-on-exec.
fn open(rows: u16, cols: u16) -> io::Result<(File, File)> {
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
            &raw mut size,
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
fn control_from_stdin(command: &mut Command) {
    // SAFETY: the closure uses only async-signal-safe syscalls between fork and
    // exec. Stdio has been attached by Command before this closure executes.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(super) fn spawn(mut command: Command, rows: u16, cols: u16) -> io::Result<Session> {
    let (master, slave) = open(rows, cols).map_err(crate::error::setup_failed)?;
    command.stdin(Stdio::from(slave.try_clone()?));
    command.stdout(Stdio::from(slave.try_clone()?));
    command.stderr(Stdio::from(slave));
    control_from_stdin(&mut command);
    let mut child = Child::new(command.spawn()?);
    let setup = crate::io::input_without_waiting(&master).and_then(|()| {
        Ok((
            Output::new(master.try_clone()?)?,
            Terminal(master.try_clone()?),
        ))
    });
    match setup {
        Ok((output, terminal)) => Ok(Session {
            child,
            input: master,
            output,
            resizer: Resizer(Arc::new(terminal)),
        }),
        Err(error) => {
            child.kill(Reach::Group);
            // Reap only: the group was just terminated; an error means it is already gone.
            let _ = child.wait();
            Err(error)
        }
    }
}
