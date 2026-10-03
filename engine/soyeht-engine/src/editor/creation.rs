//! Filesystem checks for creating documents and assets inside a workspace
//! without following symbolic links out of it.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

pub(super) fn validate_relative_creation_path(relative: &Path) -> io::Result<()> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document path must be a normal relative path",
        ));
    }
    Ok(())
}

pub(super) fn create_contained_directories(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let mut current = root.to_path_buf();
    let parent = relative.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "document path has no parent")
    })?;
    for component in parent.components() {
        let Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "document folder is invalid",
            ));
        };
        let next = current.join(component);
        match fs::create_dir(&next) {
            Ok(()) => fs::File::open(&current)?.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = fs::symlink_metadata(&next)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document folder is not a regular directory",
            ));
        }
        current = next.canonicalize()?;
        if !current.starts_with(root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document folder leaves the workspace",
            ));
        }
    }
    Ok(current)
}
