//! What a project-wide search reads, usable off the Core lock (ADR 0079).

use super::{project_files::ProjectFile, service::FileSystemPort};
use crate::{errors, errors::Result};
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

/// The disk port plus a copy of one workspace's open buffers, taken under
/// the lock so a search sees unsaved edits exactly as the editor does.
pub struct ProjectReader {
    pub(super) files: Arc<dyn FileSystemPort>,
    /// Buffer text by workspace-relative path.
    pub(super) open: BTreeMap<String, String>,
}

impl ProjectReader {
    /// Every project file under `workspace_root`. It walks the tree.
    pub fn project_files(&self, workspace_root: &Path) -> Result<Vec<ProjectFile>> {
        list_project_files(&*self.files, workspace_root)
    }

    /// The open buffer's text, or else the file on disk; `None` for a file
    /// that is gone, binary, or over the editor's size limit.
    pub fn text(&self, file: &ProjectFile) -> Result<Option<String>> {
        match self.open.get(&file.relative) {
            Some(text) => Ok(Some(text.clone())),
            None => disk_text(&*self.files, file),
        }
    }
}

pub(super) fn list_project_files(
    files: &dyn FileSystemPort,
    workspace_root: &Path,
) -> Result<Vec<ProjectFile>> {
    let root = files.canonicalize(workspace_root).map_err(errors::io)?;
    files.project_files(&root).map_err(errors::io)
}

pub(super) fn disk_text(files: &dyn FileSystemPort, file: &ProjectFile) -> Result<Option<String>> {
    match files.read_text(
        &file.canonical,
        soyeht_protocol::limits::EDITOR_HISTORY_BYTES,
    ) {
        Ok(text) => Ok(Some(text)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidData | io::ErrorKind::NotFound
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(errors::io(error)),
    }
}
