use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub(super) fn set_private(path: &Path, directory: bool) -> io::Result<()> {
    let mode = if directory { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(target_os = "macos")]
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both pointers are NUL-terminated strings that outlive the call.
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both pointers are NUL-terminated strings that outlive the call,
    // and AT_FDCWD resolves them as ordinary paths.
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

pub(super) fn dir_is_owner_only(path: &Path) -> io::Result<bool> {
    Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o077 == 0)
}

pub(super) fn file_is_owner_only(file: &File) -> io::Result<bool> {
    Ok(file.metadata()?.permissions().mode() & 0o077 == 0)
}

pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

pub(super) fn readable(canonical: std::path::PathBuf) -> std::path::PathBuf {
    canonical
}

pub(super) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

pub(super) fn replace_dir_link(path: &Path, target: &Path) -> io::Result<()> {
    match fs::read_link(path) {
        Ok(current) if current == target => return Ok(()),
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::os::unix::fs::symlink(target, path)
}

pub(super) fn owner_only(options: &mut OpenOptions) -> &mut OpenOptions {
    options.mode(0o600)
}

pub(super) fn open_no_follow(options: &OpenOptions, path: &Path) -> io::Result<File> {
    options.clone().custom_flags(libc::O_NOFOLLOW).open(path)
}

/// Takes the exclusive lock of `file`; `false` when another process holds it.
pub(super) fn try_lock(file: &File) -> io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}
