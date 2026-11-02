use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub(super) fn set_private(path: &Path, directory: bool) -> io::Result<()> {
    let mode = if directory { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Makes the directory at `path` owner-only through an open handle that refused a link: the
/// directory is opened with `O_NOFOLLOW | O_DIRECTORY` and the mode is set on that handle, so a
/// link swapped in for the directory after a check is never followed and its target never
/// changed.
pub(super) fn set_private_dir_no_follow(path: &Path) -> io::Result<()> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)?;
    directory.set_permissions(fs::Permissions::from_mode(0o700))
}

/// Whether `error` is the refusal of [`set_private_dir_no_follow`] for a link or a non-directory
/// in the place of the directory.
pub(super) fn is_link_or_not_directory(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
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

/// The user this process runs as.
fn current_user() -> u32 {
    // SAFETY: geteuid has no arguments or memory preconditions.
    unsafe { libc::geteuid() }
}

/// Whether `metadata` is of something `user` owns.
fn owned_by(metadata: &fs::Metadata, user: u32) -> bool {
    metadata.uid() == user
}

/// Whether `metadata` is of something only `user` can reach: owned by `user`, and with no mode
/// bit for the group or others. Someone else's directory with mode 0700 is theirs, not ours.
fn private_to(metadata: &fs::Metadata, user: u32) -> bool {
    owned_by(metadata, user) && metadata.permissions().mode() & 0o077 == 0
}

/// Whether the current user owns what `metadata` describes.
pub(super) fn is_owned_by_current_user(metadata: &fs::Metadata) -> bool {
    owned_by(metadata, current_user())
}

pub(super) fn dir_is_owner_only(path: &Path) -> io::Result<bool> {
    Ok(private_to(&fs::symlink_metadata(path)?, current_user()))
}

pub(super) fn file_is_owner_only(file: &File) -> io::Result<bool> {
    Ok(private_to(&file.metadata()?, current_user()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch_dir as scratch;

    #[test]
    fn a_file_is_private_to_its_owner_with_no_mode_bit_for_others_and_to_no_one_else() {
        let directory = scratch("file-private-to");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("key");
        fs::write(&path, "x").unwrap();
        set_private(&path, false).unwrap();
        let file = File::open(&path).unwrap();
        let metadata = file.metadata().unwrap();
        assert!(file_is_owner_only(&file).unwrap());
        assert!(is_owned_by_current_user(&metadata));
        assert!(!private_to(&metadata, metadata.uid().wrapping_add(1)));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(!file_is_owner_only(&File::open(&path).unwrap()).unwrap());
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_directory_is_private_to_its_owner_with_no_mode_bit_for_others_and_to_no_one_else() {
        let directory = scratch("private-to");
        fs::create_dir_all(&directory).unwrap();
        set_private(&directory, true).unwrap();
        let metadata = fs::symlink_metadata(&directory).unwrap();
        let me = metadata.uid();
        assert!(private_to(&metadata, me));
        assert!(
            !private_to(&metadata, me.wrapping_add(1)),
            "another user's is not ours"
        );
        assert!(dir_is_owner_only(&directory).unwrap());
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
        assert!(!dir_is_owner_only(&directory).unwrap());
        fs::remove_dir_all(&directory).unwrap();
    }
}
