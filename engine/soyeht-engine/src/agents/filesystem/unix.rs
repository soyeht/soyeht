//! Descriptor-relative access: every component is opened relative to its parent's descriptor
//! with `O_NOFOLLOW`, so a link swapped in while the walk runs cannot redirect it.

use crate::errors::{self, Result};
use std::{
    ffi::{CString, OsStr},
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::PermissionsExt},
    },
    path::Component,
};

/// An open directory.
pub(super) struct Directory(File);

/// A file name inside a directory.
pub(super) struct Name(CString);

/// What a replacement file keeps from the file it replaces.
pub(super) struct Attributes(libc::mode_t);

pub(super) fn name(text: &OsStr) -> Result<Name> {
    CString::new(text.as_bytes())
        .map(Name)
        .map_err(|_| errors::invalid("invalid path component"))
}

fn child(parent: &File, name: &CString, flags: i32, mode: libc::mode_t) -> Result<File> {
    // SAFETY: parent owns a live directory descriptor and name is terminated.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn open_directory<'a>(
    components: impl Iterator<Item = Component<'a>>,
) -> Result<Directory> {
    let mut directory = File::open("/")?;
    for part in components {
        match part {
            Component::RootDir => (),
            Component::Normal(part) => {
                directory = child(
                    &directory,
                    &name(part)?.0,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?
            }
            _ => return Err(errors::invalid("worktree path must be canonical")),
        }
    }
    Ok(Directory(directory))
}

/// Opens an existing file for reading without waiting on a pipe or following a link.
pub(super) fn open_file(directory: &Directory, name: &Name) -> Result<File> {
    child(&directory.0, &name.0, libc::O_RDONLY | libc::O_NONBLOCK, 0)
}

pub(super) fn attributes(file: &File) -> io::Result<Attributes> {
    // `mode_t` is 16 bits on macOS and 32 on Linux; the permission bits fit in both.
    let mode = file.metadata()?.permissions().mode() & 0o777;
    Ok(Attributes(libc::mode_t::try_from(mode).unwrap_or(0o600)))
}

/// The attributes of a file that did not exist before.
pub(super) fn new_attributes() -> Attributes {
    Attributes(0o600)
}

/// Creates a file that must not exist yet.
pub(super) fn create_exclusive(directory: &Directory, name: &Name) -> Result<File> {
    child(
        &directory.0,
        &name.0,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )
}

/// Gives `temporary` the attributes, makes it durable and renames it over `target`.
pub(super) fn replace(replacement: &super::Replacement<'_>) -> io::Result<()> {
    let r = replacement;
    // SAFETY: file owns a live descriptor; only ordinary permission bits survive.
    if unsafe { libc::fchmod(r.file.as_raw_fd(), r.attributes.0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    r.file.sync_all()?;
    // SAFETY: both names are terminated and both directory descriptors live.
    if unsafe {
        libc::renameat(
            r.directory.0.as_raw_fd(),
            r.temporary.0.as_ptr(),
            r.directory.0.as_raw_fd(),
            r.target.0.as_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    r.directory.0.sync_all()
}

/// Removes this invocation's temporary file after a failure.
pub(super) fn discard(directory: &Directory, temporary: &Name) {
    // ignored: cleanup after a failure; the failure that led here is the one reported
    // SAFETY: cleanup names only this invocation's exclusive temporary file.
    let _ = unsafe { libc::unlinkat(directory.0.as_raw_fd(), temporary.0.as_ptr(), 0) };
}
