use crate::wire::{ProcessKind, Spawn};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

pub fn spawn(config: &Spawn) -> io::Result<(Child, File)> {
    if config.kind != ProcessKind::Pty {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "pipes ACP is outside slice 1",
        ));
    }
    if config.argv.is_empty()
        || config.cols == 0
        || config.rows == 0
        || config.cols > 1000
        || config.rows > 1000
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid spawn parameters",
        ));
    }
    let mut master = -1;
    let mut slave = -1;
    let mut size = libc::winsize {
        ws_row: config.rows,
        ws_col: config.cols,
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
    let mut command = Command::new(&config.argv[0]);
    command
        .args(&config.argv[1..])
        .current_dir(&config.cwd)
        .env_clear();
    // Only these inherited values are needed by shells; no engine credentials.
    for name in [
        "HOME", "USER", "LOGNAME", "PATH", "LANG", "LC_ALL", "TMPDIR", "SHELL", "ZDOTDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("TERM", "xterm-256color");
    for (name, value) in &config.env {
        if ![
            "PATH",
            "LANG",
            "LC_ALL",
            "TERM",
            "COLORTERM",
            "SHELL",
            "PS1",
        ]
        .contains(&name.as_str())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "environment variable is not allowed",
            ));
        }
        command.env(name, value);
    }
    command.stdin(Stdio::from(slave.try_clone()?));
    command.stdout(Stdio::from(slave.try_clone()?));
    command.stderr(Stdio::from(slave));
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
    let mut child = command.spawn()?;
    // SAFETY: master is owned and valid. Nonblocking I/O keeps a child's
    // stalled stdin from blocking engine reconnection or closing another pane.
    if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
        terminate_group(child.id());
        let error = io::Error::last_os_error();
        let _ = child.wait();
        return Err(error);
    }
    Ok((child, master))
}

pub fn resize(master: &File, cols: u16, rows: u16) -> io::Result<()> {
    if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid terminal size",
        ));
    }
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: the borrowed FD and immutable winsize remain valid during ioctl.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn terminate_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid)
        && pid > 1
    {
        // SAFETY: this is the session leader returned by our live Child,
        // never a PID loaded from a previous supervisor's registry.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}
