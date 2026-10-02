//! `project.tree.list` and `project.tree.watch`: the lazy project panel tree.
//!
//! A client lists one folder at a time as the person expands it, then names
//! the folders it shows. The engine follows those folders from file events
//! (`project_watch.rs`) and tells only that connection when one changes, so
//! the panel follows files created, renamed or deleted by agents, terminals or
//! other apps.

use super::{persistence::parameters, project_paths::ProjectRoot};
use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::Value;
use soyeht_protocol::{
    ProjectTreeListParams, ProjectTreeListResult, ProjectTreeWatchParams, ProjectTreeWatchResult,
    limits::{PROJECT_TREE_ENTRIES, PROJECT_TREE_WATCHES},
};
use std::{collections::BTreeMap, io, path::Path};

impl Core {
    pub fn project_tree_list(&self, params: &Value) -> Result<Value> {
        let request: ProjectTreeListParams = parameters(params, "project tree list")?;
        let project = self.project_root(&request.workspace_id)?;
        let listing = project
            .list(
                &request.path,
                request.show_ignored.unwrap_or(false),
                PROJECT_TREE_ENTRIES,
            )
            .map_err(project_error)?;
        serde_json::to_value(ProjectTreeListResult {
            workspace_id: request.workspace_id,
            path: request.path,
            entries: listing.entries,
            truncated: listing.truncated,
        })
        .map_err(|_| errors::internal("Project tree serialization failed"))
    }

    pub fn project_tree_watch(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: ProjectTreeWatchParams = parameters(params, "project tree watch")?;
        if request.paths.len() > PROJECT_TREE_WATCHES {
            return Err(errors::invalid("Too many project folders to watch"));
        }
        let project = self.project_root(&request.workspace_id)?;
        let mut folders = BTreeMap::new();
        for path in &request.paths {
            project.directory(path).map_err(project_error)?;
            self.project_watches.record_read();
            folders.insert(path.clone(), project.fingerprint(path));
        }
        let key = (connection.to_owned(), request.workspace_id.clone());
        if folders.is_empty() {
            self.project_watches.folders.remove(&key);
        } else {
            self.start_project_source(&request.workspace_id)?;
            self.project_watches.folders.insert(key, folders);
        }
        serde_json::to_value(ProjectTreeWatchResult {
            workspace_id: request.workspace_id,
            paths: request.paths,
        })
        .map_err(|_| errors::internal("Project watch serialization failed"))
    }

    pub(super) fn project_root(&self, workspace_id: &str) -> Result<ProjectRoot> {
        let workspace = model::workspace(&self.store, workspace_id)?;
        ProjectRoot::open(Path::new(&workspace.path)).map_err(errors::io)
    }
}

/// Maps confinement and naming failures to invalid parameters, keeping real
/// I/O failures distinct.
pub(super) fn project_error(error: io::Error) -> errors::Error {
    match error.kind() {
        io::ErrorKind::NotFound => errors::not_found(error.to_string()),
        io::ErrorKind::InvalidInput
        | io::ErrorKind::PermissionDenied
        | io::ErrorKind::AlreadyExists
        | io::ErrorKind::NotADirectory
        | io::ErrorKind::IsADirectory
        | io::ErrorKind::DirectoryNotEmpty => errors::invalid(error.to_string()),
        _ => errors::io(error),
    }
}
