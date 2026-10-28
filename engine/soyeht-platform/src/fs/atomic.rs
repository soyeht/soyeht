//! Replacing a file in one step: write a temporary file next to the target, make it durable,
//! then rename it over the target. A reader sees the old bytes or the new ones, never a prefix,
//! and a crash leaves one of the two.
//!
//! The temporary file is created in the target's directory (a rename never crosses a file
//! system) under a unique name with `create_new` and without following a link, so a name planted
//! beforehand is refused and never written through. It is owner-only from the moment it exists,
//! so no secret is ever on disk with a wider mode than the file it becomes. Any error removes it.

use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{OpenOptionsPrivateExt, sync_directory};

/// Start of the name of every temporary file this module creates, so a listing can hide one.
pub const TEMPORARY_PREFIX: &str = ".soyeht-write-";

/// How many names are tried before giving up; a name only collides with a leftover or a plant.
const NAME_ATTEMPTS: usize = 16;

/// True for the name of a temporary file of [`write_atomic`] or [`stage_replacement`]. It exists
/// only while a write is in flight (or after a crash), and belongs to the writer, not to the
/// folder: no listing, search or file event should show it.
pub fn is_temporary_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with(TEMPORARY_PREFIX))
}

/// Who may reach the file after the replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Owner-only: mode 0600 on Unix, a protected ACL for the current user and the system on
    /// Windows. For secrets and the engine's own state. The result is owner-only whatever the
    /// file it replaces was. On Windows the temporary file takes its directory's access list
    /// until `set_private_file` runs, and a handle another user opened in that instant keeps
    /// its access (the default share mode allows it), so callers write only into private
    /// directories (`set_private_dir`), which is what makes the guarantee hold.
    OwnerOnly,
    /// The permissions of the file being replaced, which a person owns (a document, a skill).
    /// Their permission bits on Unix and the read-only attribute on Windows; the access list and
    /// ownership are not copied. When there is no file yet, the new one gets the permissions an
    /// ordinary creation gets. On Unix the temporary file is owner-only until the permissions
    /// are set. On Windows it takes its directory's access list until then, because a file
    /// has no per-open mode there.
    KeepPermissions,
}

/// A fully written, durable temporary file that [`Staged::commit`] renames over the target. It
/// removes the temporary file when dropped uncommitted, so a caller that decides not to go on
/// (a check that failed between the write and the rename) just drops it.
#[must_use = "dropping a staged write discards it"]
pub struct Staged {
    file: File,
    target: PathBuf,
    directory: PathBuf,
    cleanup: Cleanup,
}

/// Removes the temporary file unless it was renamed away.
struct Cleanup {
    temporary: PathBuf,
    armed: bool,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = fs::remove_file(&self.temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            crate::log::failure("platform.write_atomic_cleanup_failed", &error);
        }
    }
}

impl Staged {
    /// The open temporary file, for a caller that must hold a lock on the file that is about to
    /// take the target's place (the handle stays valid after the rename).
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Renames the temporary file over the target and syncs the directory (Windows has no
    /// directory flush; its file system journals renames). Returns the handle of the file, now
    /// the target. An error leaves the target as it was and no temporary file behind, except a
    /// failed directory sync, after which the new file is already in place.
    ///
    /// # Errors
    /// Fails when the rename or the directory sync does, for example when the target is a
    /// directory, or on Windows is read-only or held open without delete sharing.
    pub fn commit(self) -> io::Result<File> {
        let Self {
            file,
            target,
            directory,
            mut cleanup,
        } = self;
        // std's rename replaces an existing file atomically on every system (`rename(2)`, and
        // `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` on Windows).
        fs::rename(&cleanup.temporary, &target)?;
        cleanup.armed = false;
        sync_directory(&directory)?;
        Ok(file)
    }
}

/// Writes `bytes` to a temporary file next to `target`, makes it durable and permissioned as
/// `mode` says, and returns it for [`Staged::commit`]. The target is untouched until then.
///
/// # Errors
/// Fails when `target` has no file name, its directory cannot be written, or the write or the
/// sync fails; the temporary file is removed.
pub fn stage_replacement(target: &Path, bytes: &[u8], mode: WriteMode) -> io::Result<Staged> {
    stage_named(target, bytes, mode, temporary_name)
}

/// Replaces the file at `target` with `bytes` in one step: [`stage_replacement`] then
/// [`Staged::commit`]. Creates the file when there is none. A link at `target` is replaced, not
/// written through.
///
/// # Errors
/// As [`stage_replacement`] and [`Staged::commit`]; on any error the target keeps its old bytes.
pub fn write_atomic(target: &Path, bytes: &[u8], mode: WriteMode) -> io::Result<()> {
    stage_replacement(target, bytes, mode)?.commit().map(drop)
}

fn stage_named(
    target: &Path,
    bytes: &[u8],
    mode: WriteMode,
    names: impl FnMut() -> String,
) -> io::Result<Staged> {
    if target.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "target has no file name",
        ));
    }
    let directory = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    // Read before the file exists so a missing target is not an error of the open below.
    let kept = match mode {
        WriteMode::OwnerOnly => None,
        WriteMode::KeepPermissions => match fs::metadata(target) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        },
    };
    let (mut file, temporary) = create_temporary(directory, mode, kept.is_some(), names)?;
    let cleanup = Cleanup {
        temporary,
        armed: true,
    };
    if mode == WriteMode::OwnerOnly && cfg!(windows) {
        // A file made on Windows takes the ACL of its directory, which is owner-only only when it
        // came from `set_private_dir`; callers of `OwnerOnly` write only into such directories,
        // and the file is still empty here.
        super::set_private_file(&cleanup.temporary)?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Some(permissions) = kept {
        file.set_permissions(permissions)?;
    }
    Ok(Staged {
        file,
        target: target.to_owned(),
        directory: directory.to_owned(),
        cleanup,
    })
}

/// Creates the temporary file under the first name of `names` nobody has taken. A name that is
/// taken, by a leftover or a planted link, is skipped and never opened.
fn create_temporary(
    directory: &Path,
    mode: WriteMode,
    keeps_a_file: bool,
    mut names: impl FnMut() -> String,
) -> io::Result<(File, PathBuf)> {
    for _ in 0..NAME_ATTEMPTS {
        let path = directory.join(names());
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        // A file that replaces one with set permissions starts owner-only and is given them once
        // written. With no file to copy, an ordinary creation sets the mode.
        if mode == WriteMode::OwnerOnly || keeps_a_file {
            options.owner_only();
        }
        match options.open_no_follow(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the temporary file",
    ))
}

/// A name no other writer of this process, or of another process, is likely to pick: the process
/// id, a counter and the clock. `create_new` is what makes a collision harmless.
fn temporary_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    format!(
        "{TEMPORARY_PREFIX}{:x}-{:x}-{nanos:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests;
