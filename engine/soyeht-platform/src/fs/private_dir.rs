//! A directory only its owner reaches: made when missing, and never a link in its place.

use std::{fmt, fs, io, path::Path};

use super::{create_private_dir, dir_is_owner_only, sync_directory, system};

/// Why a directory is not a private one.
#[derive(Debug)]
pub enum PrivateDirError {
    /// A link, or an entry that is not a directory, is where the directory belongs. It is never
    /// followed, and never replaced.
    Unsafe,
    /// A directory that others can reach.
    NotPrivate,
    Io(io::Error),
}

impl fmt::Display for PrivateDirError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsafe => formatter.write_str("a link or a file is where the directory belongs"),
            Self::NotPrivate => formatter.write_str("the directory is not owner-only"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PrivateDirError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Unsafe | Self::NotPrivate => None,
        }
    }
}

impl From<io::Error> for PrivateDirError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<PrivateDirError> for io::Error {
    fn from(error: PrivateDirError) -> Self {
        match error {
            PrivateDirError::Io(error) => error,
            PrivateDirError::Unsafe => Self::new(io::ErrorKind::InvalidInput, error.to_string()),
            PrivateDirError::NotPrivate => {
                Self::new(io::ErrorKind::PermissionDenied, error.to_string())
            }
        }
    }
}

/// Fails unless `path` is a directory, not a link, that nobody but the owner can reach.
/// It changes nothing.
///
/// # Errors
/// [`PrivateDirError::Unsafe`] for a link or a non-directory, [`PrivateDirError::NotPrivate`] for
/// a directory others can reach, [`PrivateDirError::Io`] when `path` cannot be read.
pub fn check_private_dir(path: &Path) -> Result<(), PrivateDirError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PrivateDirError::Unsafe);
    }
    if !dir_is_owner_only(path)? {
        return Err(PrivateDirError::NotPrivate);
    }
    Ok(())
}

/// Makes `path` an owner-only directory: created, with its parents, when missing; made owner-only
/// (for its children too, as [`set_private_dir`](super::set_private_dir) does) when it exists and
/// others can reach it. A link or a file in its place is refused, not followed and not replaced.
///
/// On Unix the repair goes through an open handle that refuses a link, so a link swapped in for
/// the directory after the check is not followed. On Windows it goes by path: the parent of
/// `path` must be a directory only the owner can change, as the engine's and the service's data
/// directories are.
///
/// # Errors
/// [`PrivateDirError::Unsafe`] for a link or a non-directory; [`PrivateDirError::Io`] when the
/// directory cannot be created or made private (it belongs to someone else, say).
pub fn ensure_private_dir(path: &Path) -> Result<(), PrivateDirError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty());
            if let Some(parent) = parent {
                fs::create_dir_all(parent)?;
            }
            match create_private_dir(path) {
                Ok(()) => {
                    if let Some(parent) = parent {
                        sync_directory(parent)?;
                    }
                }
                // Another writer made it in the meantime: judged below like any existing one.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    match check_private_dir(path) {
        Err(PrivateDirError::NotPrivate) => match system::set_private_dir_no_follow(path) {
            Ok(()) => check_private_dir(path),
            Err(error) if system::is_link_or_not_directory(&error) => Err(PrivateDirError::Unsafe),
            Err(error) => Err(error.into()),
        },
        checked => checked,
    }
}

#[cfg(test)]
mod tests;
