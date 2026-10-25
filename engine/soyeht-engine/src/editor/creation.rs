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
            Ok(()) => soyeht_platform::fs::sync_directory(&current)?,
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
        current = soyeht_platform::fs::canonicalize(&next)?;
        if !current.starts_with(root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document folder leaves the workspace",
            ));
        }
    }
    Ok(current)
}

/// Creates the file at `relative` below `root` holding `text`, never replacing anything: the text
/// is written beside the target and published with an exclusive link, so a failed write or a
/// crash leaves no file, and never an empty one. `AlreadyExists` when another file is there.
pub(super) fn create_text_if_absent(
    root: &Path,
    relative: &Path,
    text: &str,
) -> io::Result<PathBuf> {
    create_bytes_if_absent(root, relative, text.as_bytes())
}

/// The same for bytes, and the same text or bytes already there are accepted.
pub(super) fn create_bytes_if_absent(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    let root = soyeht_platform::fs::canonicalize(root)?;
    validate_relative_creation_path(relative)?;
    let parent = create_contained_directories(&root, relative)?;
    let filename = relative
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target filename is missing"))?;
    let target = parent.join(filename);
    crate::immutable_file::publish(&target, bytes)?;
    canonical_inside(&root, &target)
}

/// The canonical path of `target`, which must lie inside `root` (a link may not lead out).
pub(super) fn canonical_inside(root: &Path, target: &Path) -> io::Result<PathBuf> {
    let canonical = soyeht_platform::fs::canonicalize(target)?;
    if !canonical.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "document target leaves the workspace",
        ));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_created_with_its_text_at_once_and_never_replaced() {
        let root = std::env::temp_dir().join(format!("soyeht-create-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let relative = Path::new("src/main.rs");
        let target = create_text_if_absent(&root, relative, "fn main() {}\n").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "fn main() {}\n");
        // The same text is accepted again, as after an interrupted retry; another is refused.
        create_text_if_absent(&root, relative, "fn main() {}\n").unwrap();
        let refused = create_text_if_absent(&root, relative, "other\n").unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&target).unwrap(), "fn main() {}\n");
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
        assert!(create_text_if_absent(&root, Path::new("../out.rs"), "x").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
