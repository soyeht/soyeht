//! Why a save failed, as the clients are told.

use crate::errors::{self, Error};
use soyeht_protocol::{EditorSaveFailure, ErrorKind};
use std::io;

/// What a write does with a file its owner made read-only. The write puts a new file over the
/// old one, which a read-only bit does not stop, so a caller says whether the bit holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    /// Fail with `PermissionDenied` and leave the file as it is: the editor's own save.
    Refuse,
    /// Replace it, as a project replace and an agent's write always did.
    Replace,
}

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
}
