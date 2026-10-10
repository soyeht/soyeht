//! Helpers for fixtures that touch paths: absolute paths that never touch the disk, and the
//! removal of a folder that something still has files open in.

use std::{fs, path::Path, path::PathBuf};

/// `path`, written as an absolute Unix path, as an absolute path on this system. Windows needs
/// a drive letter before a path counts as absolute.
pub(crate) fn rooted(path: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:{}", path.replace('/', "\\")))
    } else {
        PathBuf::from(path)
    }
}

/// [`rooted`] as text.
pub(crate) fn rooted_text(path: &str) -> String {
    rooted(path).to_string_lossy().into_owned()
}

/// Drops `open` (a core, a store: whatever keeps files in `folder` open), then removes the
/// folder. Windows refuses to remove a folder with open files.
pub(crate) fn remove_after<T>(open: T, folder: impl AsRef<Path>) {
    drop(open);
    fs::remove_dir_all(folder).unwrap();
}
