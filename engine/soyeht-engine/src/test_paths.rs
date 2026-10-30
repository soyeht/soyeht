//! Helpers for fixtures that touch paths: absolute paths that never touch the disk, and the
//! removal of a folder that something still has files open in.

use std::{
    fs,
    path::Path,
    path::PathBuf,
    time::{Duration, Instant},
};

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
///
/// The first core a process opens starts a thread that writes the remembered launch `PATH` into
/// its data folder once the login shell has answered, which on a busy host can be as the test
/// ends. A file that lands while the folder is being removed makes the removal fail with
/// "directory not empty", so the removal is tried again until the writer is done.
pub(crate) fn remove_after<T>(open: T, folder: impl AsRef<Path>) {
    drop(open);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match fs::remove_dir_all(folder.as_ref()) {
            Ok(()) => return,
            Err(error) if Instant::now() < deadline => {
                assert!(
                    error.kind() == std::io::ErrorKind::DirectoryNotEmpty,
                    "cannot remove {}: {error}",
                    folder.as_ref().display()
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("cannot remove {}: {error}", folder.as_ref().display()),
        }
    }
}
