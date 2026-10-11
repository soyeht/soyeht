//! Standard error as descriptor 2.

use std::{
    fs::File,
    io::{self, PipeWriter},
    os::fd::{AsFd, AsRawFd, OwnedFd},
    path::PathBuf,
};

/// A saved copy of what standard error was.
pub(super) type Original = OwnedFd;

pub(super) fn current_stderr() -> io::Result<Original> {
    io::stderr().as_fd().try_clone_to_owned()
}

/// Makes `writer` the process's standard error.
pub(super) fn redirect_stderr(writer: &PipeWriter) -> io::Result<()> {
    set_stderr(writer.as_fd())
}

/// Puts back what standard error was.
pub(super) fn restore_stderr(original: &Original) -> io::Result<()> {
    set_stderr(original.as_fd())
}

fn set_stderr(target: std::os::fd::BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: both descriptors are open for the whole call; `dup2` replaces
    // descriptor 2 atomically and leaves `target` untouched.
    let result = unsafe { libc::dup2(target.as_raw_fd(), libc::STDERR_FILENO) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn descriptor_path(file: &File) -> Option<PathBuf> {
    use std::{ffi::CStr, os::unix::ffi::OsStrExt};
    let mut buffer = [0_u8; libc::PATH_MAX as usize];
    // SAFETY: `F_GETPATH` writes a NUL-terminated path of at most `PATH_MAX`
    // bytes into `buffer`, which has exactly that size, for an open descriptor.
    let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
    if status < 0 {
        return None;
    }
    // No NUL means no usable path; rotation is then off (the caller treats None as that).
    let path = CStr::from_bytes_until_nul(&buffer).ok()?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

#[cfg(target_os = "linux")]
pub(super) fn descriptor_path(file: &File) -> Option<PathBuf> {
    match std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())) {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!("log rotation is off: cannot name the log file: {error}");
            None
        }
    }
}
