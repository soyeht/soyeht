//! Owner-only files and directories, and opens that never follow a link.
//!
//! On Unix "owner-only" is mode 0700 for a directory and 0600 for a file. On Windows it is a
//! protected ACL that grants access to the current user and the system only; a file created
//! inside such a directory inherits it.

use std::{
    fs::{File, OpenOptions},
    io::{self, Read as _, Write as _},
    path::Path,
};

mod atomic;
mod private_dir;
mod publish;
#[cfg(any(windows, test))]
mod sddl;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub use atomic::{
    Staged, TEMPORARY_PREFIX, WriteMode, is_temporary_name, stage_replacement, write_atomic,
};

pub use private_dir::{PrivateDirError, check_private_dir, ensure_private_dir};
pub use publish::publish_new;

#[cfg(unix)]
use unix as system;
#[cfg(windows)]
use windows as system;

/// Makes the directory at `path` owner-only, for its children too.
pub fn set_private_dir(path: &Path) -> io::Result<()> {
    system::set_private(path, true)
}

/// Makes the file at `path` owner-only.
pub fn set_private_file(path: &Path) -> io::Result<()> {
    system::set_private(path, false)
}

/// Renames `from` to `to`, failing with [`io::ErrorKind::AlreadyExists`] instead of replacing
/// an entry that is already there. The check and the rename are one step, so a concurrent
/// creation of `to` is never overwritten.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    system::rename_no_replace(from, to)
}

/// Creates the directory at `path`, owner-only. Its parent must exist.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    system::create_private_dir(path)
}

/// Whether nobody but the current user (and, on Windows, the system and administrators) can reach
/// the directory at `path`: on Unix it is owned by that user and has no mode bit for others. A
/// link is judged as itself.
pub fn dir_is_owner_only(path: &Path) -> io::Result<bool> {
    system::dir_is_owner_only(path)
}

/// Whether nobody but the current user (and, on Windows, the system and administrators) can
/// reach the open file; on Unix it is owned by that user as well as having no mode bit for others.
pub fn file_is_owner_only(file: &File) -> io::Result<bool> {
    system::file_is_owner_only(file)
}

/// Flushes a directory's entries to disk after a rename into it. Windows has no directory
/// flush; its file system journals renames, so there it does nothing.
pub fn sync_directory(path: &Path) -> io::Result<()> {
    system::sync_directory(path)
}

/// Creates a symbolic link at `link` pointing at `target`. Windows makes a directory link when
/// `target` is a directory and a file link otherwise, and the account needs the right to create
/// symbolic links (Developer Mode, or an elevated process).
pub fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    system::symlink(target.as_ref(), link.as_ref())
}

/// The canonical form of `path`, as [`std::fs::canonicalize`] gives it, written the way a
/// person and a client read it: Windows' extended `\\?\C:\...` form becomes `C:\...`.
pub fn canonicalize(path: &Path) -> io::Result<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(system::readable(canonical))
}

/// Why a process could not become the only one holding a lock.
#[derive(Debug)]
pub enum LockError {
    /// Another process holds it.
    Held,
    Io(io::Error),
}

impl From<io::Error> for LockError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Makes this process the only one running for `directory`: creates the owner-only directory
/// and takes an exclusive lock on `name` inside it. The lock lasts as long as the returned file
/// stays open, and the system drops it if the process ends.
pub fn lock_singleton(directory: &Path, name: &str) -> Result<File, LockError> {
    std::fs::create_dir_all(directory)?;
    set_private_dir(directory)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .owner_only()
        .open_no_follow(&directory.join(name))?;
    if system::try_lock(&lock)? {
        // The record only lets an uninstall find the program; a program that cannot write it
        // still runs, and the uninstall then says it could not end it.
        if let Err(error) = record_process(&lock) {
            crate::log::failure("platform.lock_record_failed", &error);
        }
        Ok(lock)
    } else {
        Err(LockError::Held)
    }
}

/// Writes the id of this process over the contents of a lock it holds, so that whoever must end
/// the program (an uninstall) can find it from the lock alone ([`singleton_process`]).
fn record_process(mut lock: &File) -> io::Result<()> {
    lock.set_len(0)?;
    lock.write_all(std::process::id().to_string().as_bytes())
}

/// The names of the locks the engine and the supervisor take with [`lock_singleton`] in their data
/// directory, so that whoever asks whether one runs reads the same file.
pub const ENGINE_LOCK: &str = "engine.lock";
pub const PTYD_LOCK: &str = "ptyd.lock";

/// Whether another process holds the singleton lock `name` in `directory`: a program that
/// [`lock_singleton`]ed it and still runs, whoever started it. A lock that does not exist is not
/// held. A free lock is taken for an instant and given back, and nothing is created.
pub fn singleton_held(directory: &Path, name: &str) -> io::Result<bool> {
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .open_no_follow(&directory.join(name))
    {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(!system::try_lock(&lock)?)
}

/// The id of the process that holds the singleton lock `name` in `directory`, which that process
/// wrote when it took the lock; `None` when nobody holds it or the id cannot be read. The lock
/// is the proof that the process is alive and is the one that wrote its id, so a record left by
/// a program that ended is never returned. A program that has taken the lock and not yet written
/// its id would show the id of the one before it, so the id is read twice, a moment apart, and
/// only an id that both reads agree on, with the lock held both times, is returned.
pub fn singleton_process(directory: &Path, name: &str) -> io::Result<Option<u32>> {
    let read = || -> io::Result<Option<u32>> {
        if !singleton_held(directory, name)? {
            return Ok(None);
        }
        // The file the lock is on, opened the way `singleton_held` opens it.
        let mut record = String::new();
        OpenOptions::new()
            .read(true)
            .open_no_follow(&directory.join(name))?
            .read_to_string(&mut record)?;
        let record = record.trim();
        // A program of a build that wrote no record holds the lock over an empty file.
        if record.is_empty() {
            return Ok(None);
        }
        record
            .parse()
            .map(Some)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    };
    let Some(first) = read()? else {
        return Ok(None);
    };
    std::thread::sleep(RECORD_SETTLE);
    Ok((read()? == Some(first)).then_some(first))
}

/// The file an uninstall leaves in the data directory, so that a window that was open when it ran
/// does not start the programs again ([`mark_uninstalled`]).
pub const UNINSTALL_MARKER: &str = "uninstalled";

/// Records that an uninstall is ending the programs of `directory` now: a file holding a value
/// that no other uninstall writes. A window reads it when it begins ([`uninstall_record`]) and
/// takes an uninstall to have happened when the value has changed since, so no clock is
/// compared. Does nothing when the directory does not exist (nothing was ever started there).
pub fn mark_uninstalled(directory: &Path) -> io::Result<()> {
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if !directory.is_dir() {
        return Ok(());
    }
    // Unique among uninstalls: this process, the time to the nanosecond and a count. The time is
    // not compared with anything, only made part of the value.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let nonce = format!(
        "{}-{nanos}-{}\n",
        std::process::id(),
        COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .owner_only()
        .open_no_follow(&directory.join(UNINSTALL_MARKER))?
        .write_all(nonce.as_bytes())
}

/// The value an uninstall left in `directory`, or `None` when there is none.
pub fn uninstall_record(directory: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(directory.join(UNINSTALL_MARKER)) {
        Ok(record) => Ok(Some(record)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Takes the record of an uninstall away: a registration written again, or an engine started
/// again, is a new start.
pub fn clear_uninstalled(directory: &Path) -> io::Result<()> {
    match std::fs::remove_file(directory.join(UNINSTALL_MARKER)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// How long [`singleton_process`] waits between its two reads: far longer than a program takes
/// to write its id after it takes the lock.
const RECORD_SETTLE: std::time::Duration = std::time::Duration::from_millis(50);

/// Takes the exclusive lock `lock_singleton` takes, on a file that is already open; `false` when
/// another holder has it. The lock lasts as long as `file` stays open and leaves the contents of
/// the file readable.
#[cfg(any(windows, test))]
pub(crate) fn try_lock(file: &File) -> io::Result<bool> {
    system::try_lock(file)
}

/// Points the directory link `path` at `target`, replacing a link that points elsewhere and
/// creating missing parents. A relative `target` is read from the folder holding `path`, as a
/// symbolic link reads it.
///
/// Unix makes a symbolic link. Windows makes a symbolic link when the account may create one
/// and a directory junction otherwise; a junction needs an existing target, which is created
/// empty when missing.
pub fn replace_dir_link(path: &Path, target: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    system::replace_dir_link(path, target)
}

/// Opens that are owner-only when they create and refuse a link at the last component.
pub trait OpenOptionsPrivateExt {
    /// A file this open creates is owner-only (mode 0600). On Windows the file takes the ACL of
    /// its directory, which is owner-only when the directory came from [`set_private_dir`].
    fn owner_only(&mut self) -> &mut Self;

    /// Opens `path`, failing instead of following a symbolic link or junction at its last
    /// component.
    fn open_no_follow(&self, path: &Path) -> io::Result<File>;
}

impl OpenOptionsPrivateExt for OpenOptions {
    fn owner_only(&mut self) -> &mut Self {
        system::owner_only(self)
    }

    fn open_no_follow(&self, path: &Path) -> io::Result<File> {
        system::open_no_follow(self, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::scratch_dir as scratch;

    #[test]
    fn private_directories_are_created_and_files_open_without_following_links() {
        let directory = scratch("private");
        std::fs::create_dir_all(directory.join("a/b")).unwrap();
        set_private_dir(&directory).unwrap();
        let file = directory.join("a/b/file");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .owner_only()
            .open_no_follow(&file)
            .unwrap();
        set_private_file(&file).unwrap();
        sync_directory(&directory.join("a/b")).unwrap();
        OpenOptions::new().read(true).open_no_follow(&file).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A private folder holding a private file, both checked.
    fn private_file(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let directory = scratch(name);
        std::fs::create_dir_all(&directory).unwrap();
        set_private_dir(&directory).unwrap();
        assert!(dir_is_owner_only(&directory).unwrap());
        let file = directory.join("file");
        std::fs::write(&file, "x").unwrap();
        set_private_file(&file).unwrap();
        assert!(file_is_owner_only(&File::open(&file).unwrap()).unwrap());
        (directory, file)
    }

    #[test]
    fn what_set_private_makes_passes_the_owner_only_check() {
        let (directory, _) = private_file("owner-only");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_null_access_list_fails_the_owner_only_check() {
        let (directory, file) = private_file("null-dacl");
        system::set_access_list(&file, "D:NO_ACCESS_CONTROL").unwrap();
        assert!(!file_is_owner_only(&File::open(&file).unwrap()).unwrap());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_to_a_name_the_command_interpreter_would_expand_reaches_that_folder() {
        let directory = scratch("junction");
        let target = directory.join("100%PATH%&(x)^y");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("inside"), "found").unwrap();
        let link = directory.join("link");
        system::junction(&link, &target).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(link.join("inside")).unwrap(),
            "found"
        );
        std::fs::remove_dir(&link).unwrap();
        assert!(target.join("inside").is_file());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_refused() {
        let directory = scratch("link");
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("target");
        std::fs::write(&target, "x").unwrap();
        let link = directory.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(OpenOptions::new().read(true).open_no_follow(&link).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_singleton_lock_is_held_only_while_a_program_holds_it() {
        let _serial = crate::testing::serialize_processes();
        let directory = scratch("singleton-held");
        std::fs::create_dir_all(&directory).unwrap();
        // No lock file: nothing runs, and asking creates nothing.
        assert!(!singleton_held(&directory, ENGINE_LOCK).unwrap());
        assert!(!directory.join(ENGINE_LOCK).exists());
        let lock = lock_singleton(&directory, ENGINE_LOCK).unwrap();
        assert!(singleton_held(&directory, ENGINE_LOCK).unwrap());
        // Another program's lock is another file.
        assert!(!singleton_held(&directory, PTYD_LOCK).unwrap());
        drop(lock);
        // The file stays after the program ends; the lock does not.
        assert!(directory.join(ENGINE_LOCK).is_file());
        assert!(!singleton_held(&directory, ENGINE_LOCK).unwrap());
        // Asking took no lock for good: a program can take it now.
        drop(lock_singleton(&directory, ENGINE_LOCK).unwrap());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_holder_of_a_lock_is_found_by_the_id_it_wrote() {
        let _serial = crate::testing::serialize_processes();
        let directory = scratch("singleton-process");
        std::fs::create_dir_all(&directory).unwrap();
        // Nothing holds it yet: no file, then a record left by a program that ended.
        assert_eq!(singleton_process(&directory, ENGINE_LOCK).unwrap(), None);
        std::fs::write(directory.join(ENGINE_LOCK), "4242").unwrap();
        assert_eq!(singleton_process(&directory, ENGINE_LOCK).unwrap(), None);
        let lock = lock_singleton(&directory, ENGINE_LOCK).unwrap();
        // Taking the lock replaced the old record, and a longer one leaves nothing of itself.
        assert_eq!(
            singleton_process(&directory, ENGINE_LOCK).unwrap(),
            Some(std::process::id())
        );
        assert_eq!(singleton_process(&directory, PTYD_LOCK).unwrap(), None);
        drop(lock);
        assert_eq!(singleton_process(&directory, ENGINE_LOCK).unwrap(), None);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_lock_over_an_empty_or_unreadable_record_names_no_process() {
        let _serial = crate::testing::serialize_processes();
        let directory = scratch("singleton-record");
        std::fs::create_dir_all(&directory).unwrap();
        // A program of a build that wrote no record: held, and nothing to ask.
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(directory.join(ENGINE_LOCK))
            .unwrap();
        assert!(system::try_lock(&lock).unwrap());
        assert_eq!(singleton_process(&directory, ENGINE_LOCK).unwrap(), None);
        // Something that is not an id is an error, not a guess.
        (&lock).write_all(b"not a number").unwrap();
        assert_eq!(
            singleton_process(&directory, ENGINE_LOCK)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(lock);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn each_uninstall_leaves_a_value_of_its_own() {
        let directory = scratch("uninstall-marker");
        assert_eq!(uninstall_record(&directory).unwrap(), None);
        mark_uninstalled(&directory).unwrap();
        let first = uninstall_record(&directory).unwrap().unwrap();
        mark_uninstalled(&directory).unwrap();
        let second = uninstall_record(&directory).unwrap().unwrap();
        assert_ne!(first, second);
        clear_uninstalled(&directory).unwrap();
        assert_eq!(uninstall_record(&directory).unwrap(), None);
        // Clearing what is not there, and marking a folder that does not exist, change nothing.
        clear_uninstalled(&directory).unwrap();
        let missing = directory.join("missing");
        mark_uninstalled(&missing).unwrap();
        assert!(!missing.exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
