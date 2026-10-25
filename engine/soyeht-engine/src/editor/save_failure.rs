//! Why a save failed, as the clients are told, and the one rule that decides whether a file
//! may be written over.

use crate::errors::{self, Error};
use soyeht_protocol::{EditorSaveFailure, ErrorKind};
use std::{fs, io, path::Path};

/// The class of a failed save that a person can act on: the file or its folder is gone, the
/// system refuses the write, the disk has no room, or anything else. "No room" is the engine's
/// own `DiskFull` (the same one `errors::kind` gives a full database), so one rule says it.
pub(super) fn classify(error: &Error) -> EditorSaveFailure {
    if errors::kind(error) == ErrorKind::DiskFull {
        return EditorSaveFailure::Full;
    }
    match error {
        Error::Io(error) => classify_io(error.kind()),
        _ => EditorSaveFailure::Other,
    }
}

fn classify_io(kind: io::ErrorKind) -> EditorSaveFailure {
    match kind {
        io::ErrorKind::NotFound => EditorSaveFailure::Missing,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            EditorSaveFailure::Denied
        }
        io::ErrorKind::QuotaExceeded => EditorSaveFailure::Full,
        _ => EditorSaveFailure::Other,
    }
}

/// Whether `error` says that the file is not there: a save of a file the person asked for may
/// then create it again.
pub(super) fn is_missing(error: &Error) -> bool {
    matches!(error, Error::Io(error) if error.kind() == io::ErrorKind::NotFound)
}

/// The editor's save does not replace a file its owner made read-only. The save writes a new
/// file and renames it over the old one, which a read-only bit does not stop, so the refusal is
/// made here, for the editor's autosave and `file.save` only: a project replace and an agent's
/// write are not the person's save and keep replacing such a file, as they always did.
pub(super) fn refuse_read_only(path: &Path) -> io::Result<()> {
    if fs::metadata(path)?.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is read-only",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors;

    #[test]
    fn each_class_of_io_failure_is_told_apart() {
        for (kind, expected) in [
            (io::ErrorKind::NotFound, EditorSaveFailure::Missing),
            (io::ErrorKind::PermissionDenied, EditorSaveFailure::Denied),
            (io::ErrorKind::ReadOnlyFilesystem, EditorSaveFailure::Denied),
            (io::ErrorKind::StorageFull, EditorSaveFailure::Full),
            (io::ErrorKind::QuotaExceeded, EditorSaveFailure::Full),
            (io::ErrorKind::TimedOut, EditorSaveFailure::Other),
        ] {
            assert_eq!(classify(&errors::io(kind.into())), expected, "{kind:?}");
        }
    }

    #[test]
    fn a_full_database_is_told_as_a_full_disk() {
        assert_eq!(
            classify(&errors::storage(soyeht_store::Error::DiskFull)),
            EditorSaveFailure::Full
        );
        assert_eq!(
            classify(&errors::disk_full("no room")),
            EditorSaveFailure::Full
        );
    }

    #[test]
    fn a_failure_that_is_not_io_is_other() {
        assert_eq!(
            classify(&errors::invalid("File path must stay within the workspace")),
            EditorSaveFailure::Other
        );
    }

    #[test]
    fn a_read_only_file_is_refused_and_a_writable_one_is_not() {
        let directory = std::env::temp_dir().join(format!("soyeht-ro-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("a.txt");
        fs::write(&path, "a").unwrap();
        assert!(refuse_read_only(&path).is_ok());
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(
            refuse_read_only(&path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        fs::remove_dir_all(&directory).unwrap();
    }
}
