//! Publishing a file that must not replace anything: prepared beside its target, made durable,
//! then linked into place, so a reader never sees partial bytes and a retry never overwrites.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

use super::{
    OpenOptionsPrivateExt, WriteMode,
    atomic::{Cleanup, create_temporary, directory_of, temporary_name},
    file_is_owner_only, set_private_file, sync_directory,
};

/// Publishes `bytes` at `target`, whose directory exists, and never replaces an entry.
///
/// The bytes are written to a temporary file in the directory (its name starts with
/// [`TEMPORARY_PREFIX`](super::TEMPORARY_PREFIX)), synced, and linked to `target` with an
/// exclusive hard link, so a file with missing bytes is never visible, not even after a crash. A
/// regular file already at `target` with the same bytes is accepted, as after an interrupted
/// retry, before anything is written, so it works in a directory that has become read-only; any
/// other entry, a link included, fails with [`io::ErrorKind::AlreadyExists`]. The temporary file
/// is removed on every path.
///
/// `mode` says who may reach the file: [`WriteMode::OwnerOnly`] makes it owner-only from the
/// moment it exists and accepts an existing file only when that is owner-only too;
/// [`WriteMode::KeepPermissions`] gives it the permissions an ordinary creation gets.
///
/// Where the link fails for a reason other than an existing target (FAT and exFAT volumes, some
/// network shares: the error is not one kind across systems) the temporary file is renamed to
/// the target when nothing is there. That is atomic too; what it gives up is the exclusive link,
/// so a program that creates the target in the instant between the check and the rename is
/// overwritten, which a plain save of an editor does as well. The failure of the link is logged
/// by kind.
///
/// # Errors
/// Fails when `target` has no file name, the temporary file cannot be written, `target` holds
/// something else, or neither the link nor the rename works.
pub fn publish_new(target: &Path, bytes: &[u8], mode: WriteMode) -> io::Result<()> {
    publish_linking(target, bytes, mode, |from, to| fs::hard_link(from, to))
}

fn publish_linking(
    target: &Path,
    bytes: &[u8],
    mode: WriteMode,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let directory = directory_of(target)?;
    // A retry finds the file it published: accepted before anything is written, so it works in a
    // directory that has become read-only, and it leaves no temporary file to clean up.
    match fs::symlink_metadata(target) {
        Ok(_) => return verify_existing(target, bytes, mode),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let (mut file, temporary) = create_temporary(directory, mode, false, temporary_name)?;
    // Removes the temporary file on every path, a successful link included; a rename takes it
    // away first and then there is nothing to remove.
    let cleanup = Cleanup {
        temporary,
        armed: true,
    };
    let published = (|| {
        if mode == WriteMode::OwnerOnly && cfg!(windows) {
            // A file made on Windows takes the ACL of its directory; it is still empty here.
            set_private_file(&cleanup.temporary)?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        // Closed before the link or the rename: Windows will not move an open file.
        drop(file);
        match link(&cleanup.temporary, target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                verify_existing(target, bytes, mode)
            }
            // Any other failure of the link: what a file system without hard links answers is not
            // one kind (not permitted on Linux FAT, errno 45 on macOS FAT and exFAT, an
            // uncategorized code on Windows), so the rename tries, and a real cause fails it too.
            Err(error) => {
                crate::log::failure("platform.publish_link_failed", &error);
                rename_if_absent(&cleanup.temporary, target, bytes, mode)
            }
        }
    })();
    drop(cleanup);
    published?;
    sync_directory(directory)
}

fn rename_if_absent(
    temporary: &Path,
    target: &Path,
    bytes: &[u8],
    mode: WriteMode,
) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(_) => verify_existing(target, bytes, mode),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::rename(temporary, target),
        Err(error) => Err(error),
    }
}

/// Accepts what is at `target` when it is a regular file, not a link, that holds `bytes` (and is
/// owner-only when the mode says so).
fn verify_existing(target: &Path, bytes: &[u8], mode: WriteMode) -> io::Result<()> {
    let different = || {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target already holds different content",
        )
    };
    let metadata = fs::symlink_metadata(target)?;
    let expected_len = u64::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "content is too large"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected_len {
        return Err(different());
    }
    let mut existing: File = fs::OpenOptions::new().read(true).open_no_follow(target)?;
    if mode == WriteMode::OwnerOnly && !file_is_owner_only(&existing)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the existing file is not owner-only",
        ));
    }
    let mut prior = Vec::with_capacity(bytes.len());
    Read::by_ref(&mut existing)
        .take(expected_len.saturating_add(1))
        .read_to_end(&mut prior)?;
    if prior != bytes {
        return Err(different());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
