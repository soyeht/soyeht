//! Moves project entries to the system trash.
//!
//! macOS uses `NSFileManager.trashItemAtURL`, the same call Finder uses, so
//! the item can be put back. Other systems report `Unsupported`, and the
//! caller asks the person before deleting permanently.

use std::{io, path::Path};

#[cfg(target_os = "macos")]
pub(crate) fn move_to_trash(path: &Path, directory: bool) -> io::Result<()> {
    use objc2_foundation::{NSFileManager, NSURL};
    objc2::rc::autoreleasepool(|_| {
        let url = if directory {
            NSURL::from_directory_path(path)
        } else {
            NSURL::from_file_path(path)
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "project path cannot become a file URL",
            )
        })?;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    error.localizedDescription().to_string(),
                )
            })
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn move_to_trash(_path: &Path, _directory: bool) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this system has no trash the engine can use",
    ))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// Puts a real (empty) file in the person's Trash, so it runs only on request:
    /// `cargo test -p soyeht-engine trash -- --ignored`.
    #[test]
    #[ignore]
    fn a_file_moves_to_the_system_trash() {
        let path =
            std::env::temp_dir().join(format!("soyeht-trash-check-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, "").unwrap();
        move_to_trash(&path, false).unwrap();
        assert!(!path.exists());
    }
}
