//! Putting an installed tree in place, and checking it is still what was put there.

use super::Locked;
use crate::{
    errors::{Error, Result},
    streak::{Streak, first_failure, recovered},
};
use sha2::{Digest, Sha256};
use soyeht_platform::log;
use std::{
    fs::{self, File},
    io,
    path::Path,
    sync::Mutex,
};

/// The prefix of the folder `npm ci` runs in, before the tree is moved into place.
pub(super) const STAGING: &str = ".staging-";
/// The prefix of the folder a tree that is replaced is moved to, before it is deleted.
const ASIDE: &str = ".replaced-";
/// npm's own record of what a tree holds, below `node_modules`.
const NPM_RECORD: &str = ".package-lock.json";

/// The failure to delete a folder of ours, which repeats with each install.
static REMOVALS: Mutex<Streak> = Mutex::new(Streak::once());

/// Logs a failure to clean up after an install, once per change of kind. Nothing depends on it:
/// what could not be deleted is swept by the next install.
fn report(event: &str, error: io::Error) {
    let error = Error::from(error);
    if first_failure(&REMOVALS, &log::Failure::failure_kind(&error)) {
        log::failure(event, &error);
    }
}

/// Deletes `path` and what it holds. A failure (a file a running adapter holds open, on Windows) is
/// logged by kind and left to the next [`sweep`].
pub(super) fn remove_tree(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => recovered(&REMOVALS),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => report("engine.adapter_cleanup_failed", error),
    }
}

/// Deletes what an install that was cut short left in `root`: staging folders, and trees that a
/// replacement moved aside and could not delete. The caller holds the install lock, so none of
/// them is in use by an install of this engine.
pub(super) fn sweep(root: &Path) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => return report("engine.adapter_cleanup_failed", error),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report("engine.adapter_cleanup_failed", error);
                continue;
            }
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(STAGING) || name.starts_with(ASIDE) {
            remove_tree(&entry.path());
        }
    }
}

/// Moves the finished tree in `staging` to `folder`. A tree already at `folder` (the same lock,
/// found changed or half made) is first moved aside, not deleted in place, so the move cannot
/// fail halfway on a system that refuses to delete files a running adapter holds, and it is
/// put back when the new one cannot be moved in.
pub(super) fn replace(staging: &Path, folder: &Path, root: &Path) -> Result<()> {
    let aside = if fs::symlink_metadata(folder).is_ok() {
        let aside = root.join(format!("{ASIDE}{}", uuid::Uuid::new_v4()));
        fs::rename(folder, &aside)?;
        Some(aside)
    } else {
        None
    };
    if let Err(error) = fs::rename(staging, folder) {
        if let Some(aside) = &aside
            && let Err(restore) = fs::rename(aside, folder)
        {
            report("engine.adapter_restore_failed", restore);
        }
        return Err(error.into());
    }
    if let Some(aside) = aside {
        remove_tree(&aside);
    }
    Ok(())
}

/// Adds the bytes of the regular file `path` to `hash`, with its length first so two files cannot
/// be told apart from one. A link is refused: the tree is the engine's own and has none.
fn add_file(hash: &mut Sha256, path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    let mut file = File::open(path)?;
    hash.update(file.metadata()?.len().to_be_bytes());
    io::copy(&mut file, hash)?;
    Ok(())
}

/// A digest of what a start of the tree in `folder` runs: the adapter's entry script, its
/// manifest, and npm's record of the tree (the version and integrity of every package it holds).
/// It reads a few files, so a start can afford it; it is taken when the tree is installed and
/// compared at each start.
pub(super) fn tree_digest(locked: &Locked, folder: &Path) -> Result<String> {
    let modules = folder.join("node_modules");
    let mut hash = Sha256::new();
    add_file(&mut hash, &locked.entry_path(folder))?;
    add_file(
        &mut hash,
        &modules.join(locked.package).join("package.json"),
    )?;
    // Absent when the tree was not made by npm, which only a test does.
    match add_file(&mut hash, &modules.join(NPM_RECORD)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(format!("{:x}", hash.finalize()))
}
