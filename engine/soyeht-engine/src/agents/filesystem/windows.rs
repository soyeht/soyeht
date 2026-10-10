//! Path-based access with a check at every step. Windows has no descriptor-relative open in the
//! standard library, so each folder and the file itself is opened as the link it may be and
//! refused when it is one; a folder swapped for a link between two steps is the one race this
//! cannot close.

use crate::errors::{self, Result};
use soyeht_platform::fs::OpenOptionsPrivateExt;
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io,
    path::{Component, PathBuf},
};

/// A folder that was a real folder, not a link, when it was opened.
pub(super) struct Directory(PathBuf);

/// A file name inside a directory.
pub(super) struct Name(OsString);

/// What a replacement file keeps from the file it replaces: whether it is read-only.
pub(super) struct Attributes {
    read_only: bool,
}

pub(super) fn name(text: &OsStr) -> Result<Name> {
    if text.is_empty() {
        return Err(errors::invalid("invalid path component"));
    }
    Ok(Name(text.to_owned()))
}

pub(super) fn open_directory<'a>(
    components: impl Iterator<Item = Component<'a>>,
) -> Result<Directory> {
    let mut path = PathBuf::new();
    for part in components {
        match part {
            Component::Prefix(_) | Component::RootDir => path.push(part.as_os_str()),
            Component::Normal(part) => {
                path.push(part);
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "a folder on the path is a link or not a folder",
                    )
                    .into());
                }
            }
            _ => return Err(errors::invalid("worktree path must be canonical")),
        }
    }
    Ok(Directory(path))
}

/// Opens an existing file for reading, refusing a link.
pub(super) fn open_file(directory: &Directory, name: &Name) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .open_no_follow(&directory.0.join(&name.0))?)
}

pub(super) fn attributes(file: &File) -> io::Result<Attributes> {
    Ok(Attributes {
        read_only: file.metadata()?.permissions().readonly(),
    })
}

/// The attributes of a file that did not exist before.
pub(super) fn new_attributes() -> Attributes {
    Attributes { read_only: false }
}

/// Creates a file that must not exist yet.
pub(super) fn create_exclusive(directory: &Directory, name: &Name) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .open_no_follow(&directory.0.join(&name.0))?)
}

/// Gives `temporary` the attributes, makes it durable and renames it over `target`.
pub(super) fn replace(replacement: &super::Replacement<'_>) -> io::Result<()> {
    let r = replacement;
    r.file.sync_all()?;
    let source = r.directory.0.join(&r.temporary.0);
    if r.attributes.read_only {
        let mut permissions = r.file.metadata()?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&source, permissions)?;
    }
    // A rename replaces the file at the target by a new name for the new content, so another
    // hard link to the old content is left alone. A read-only target is refused by the system;
    // the person made it read-only and the write stops there.
    fs::rename(source, r.directory.0.join(&r.target.0))
}

/// Removes this invocation's temporary file after a failure.
pub(super) fn discard(directory: &Directory, temporary: &Name) {
    if let Err(error) = fs::remove_file(directory.0.join(&temporary.0))
        && error.kind() != io::ErrorKind::NotFound
    {
        eprintln!(
            "{}",
            serde_json::json!({"event":"engine.agent_write_cleanup_failed","kind":format!("{:?}", error.kind())})
        );
    }
}
