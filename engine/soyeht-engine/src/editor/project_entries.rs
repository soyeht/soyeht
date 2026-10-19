//! `project.entry.create`, `project.entry.rename` and `project.entry.delete`.
//!
//! The filesystem change happens first, confined to the workspace by
//! `ProjectRoot`; the mutation that follows records the result and moves or
//! closes the editor files that lived at the old path. A rename keeps every
//! open buffer, draft and local history attached to its file. A delete saves
//! pending drafts first, refuses while a draft conflicts with the disk, and
//! closes the tabs of what it removed.

use super::{persistence::parameters, project_paths::normalized, project_tree::project_error};
use crate::{
    core::{Core, persist_pane},
    errors::{self, Result},
    model,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    PaneMode, ProjectDeleteOutcome, ProjectEntryCreateParams, ProjectEntryCreateResult,
    ProjectEntryDeleteParams, ProjectEntryDeleteResult, ProjectEntryRenameParams,
    ProjectEntryRenameResult, event, method,
};
use soyeht_store::editor::{EditorFile, document};
use std::path::Path;

impl Core {
    pub fn project_entry_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: ProjectEntryCreateParams = parameters(params, "project entry create")?;
        let entry = self
            .project_root(&request.workspace_id)?
            .create(&request.path, request.kind)
            .map_err(project_error)?;
        let result = serde_json::to_value(ProjectEntryCreateResult {
            workspace_id: request.workspace_id,
            entry,
        })
        .map_err(|_| errors::internal("Project entry serialization failed"))?;
        Ok(self
            .mutate(connection, method::PROJECT_ENTRY_CREATE, params, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }

    pub fn project_entry_rename(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: ProjectEntryRenameParams = parameters(params, "project entry rename")?;
        let from = normalized(&request.from).map_err(project_error)?;
        let to = normalized(&request.to).map_err(project_error)?;
        if !self
            .live_files_under(&request.workspace_id, &to)?
            .is_empty()
        {
            return Err(errors::invalid(
                "An open editor still owns the destination path",
            ));
        }
        let moving = self.live_files_under(&request.workspace_id, &from)?;
        self.refuse_open_writing(&moving)?;
        self.project_root(&request.workspace_id)?
            .rename(&from, &to)
            .map_err(project_error)?;
        let now = self.now_ms();
        let mut files = Vec::new();
        let mut panes = Vec::new();
        for file in moving {
            let Ok(mut moved) = self.editor.stored(&file.file_id).cloned() else {
                continue;
            };
            moved.relative_path = format!("{to}{}", &file.relative_path[from.len()..]);
            moved.language = super::language(Path::new(&moved.relative_path));
            moved.updated_ms = now;
            if let Some(pane) = moved
                .pane_id
                .as_deref()
                .and_then(|pane_id| self.panes.get(pane_id))
                .map(|running| running.stored.clone())
                .filter(|_| file.relative_path == from)
            {
                panes.push(renamed_pane(
                    pane,
                    &file.relative_path,
                    &moved.relative_path,
                ));
            }
            files.push(moved);
        }
        let documents = files
            .iter()
            .map(document)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(errors::storage)?;
        let result = serde_json::to_value(ProjectEntryRenameResult {
            workspace_id: request.workspace_id.clone(),
            from: from.clone(),
            to: to.clone(),
            files: documents,
        })
        .map_err(|_| errors::internal("Project rename serialization failed"))?;
        let workspace_id = request.workspace_id.clone();
        let mutation = self.mutate(connection, method::PROJECT_ENTRY_RENAME, params, |change| {
            change.move_editor_paths(&workspace_id, &from, &to)?;
            let mut events = Vec::new();
            for file in &files {
                change.save_editor_file(file)?;
            }
            for pane in &panes {
                persist_pane(change, pane)?;
                events.push((event::PANE_RENAMED.into(), json!({"pane":pane.pane})));
            }
            events.push((event::PROJECT_ENTRY_RENAMED.into(), result.clone()));
            events.push(super::tabs::refresh_tabs(change, &workspace_id)?);
            Ok((result, events))
        })?;
        if !mutation.repeated {
            for file in files {
                self.track_language_file(&file);
                self.editor.replace(file, now);
            }
            for pane in panes {
                if let Some(running) = self.panes.get_mut(&pane.pane.pane_id) {
                    running.stored = pane;
                }
            }
        }
        Ok(mutation.result)
    }

    pub fn project_entry_delete(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: ProjectEntryDeleteParams = parameters(params, "project entry delete")?;
        let path = normalized(&request.path).map_err(project_error)?;
        let project = self.project_root(&request.workspace_id)?;
        project.entry(&path).map_err(project_error)?;
        let open = self.live_files_under(&request.workspace_id, &path)?;
        self.refuse_open_writing(&open)?;
        for file in &open {
            self.save_editor_now(&file.file_id)?;
        }
        if let Some(file) = open.iter().find(|file| {
            self.editor
                .stored(&file.file_id)
                .is_ok_and(|current| current.draft_text.is_some())
        }) {
            return Err(errors::invalid(format!(
                "{} has unsaved changes; resolve them before deleting",
                file.relative_path
            )));
        }
        let outcome = if request.permanent == Some(true) {
            project.remove(&path).map_err(project_error)?;
            ProjectDeleteOutcome::Deleted
        } else {
            match project.trash(&path) {
                Ok(()) => ProjectDeleteOutcome::Trashed,
                Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {
                    ProjectDeleteOutcome::NeedsConfirmation
                }
                Err(error) => return Err(project_error(error)),
            }
        };
        let result = serde_json::to_value(ProjectEntryDeleteResult {
            workspace_id: request.workspace_id.clone(),
            path,
            outcome: outcome.clone(),
        })
        .map_err(|_| errors::internal("Project delete serialization failed"))?;
        if outcome == ProjectDeleteOutcome::NeedsConfirmation {
            return Ok(self
                .mutate(connection, method::PROJECT_ENTRY_DELETE, params, |_| {
                    Ok((result, Vec::new()))
                })?
                .result);
        }
        self.close_deleted_panes(connection, params, &request.workspace_id, &open, result)
    }

    fn close_deleted_panes(
        &mut self,
        connection: &str,
        params: &Value,
        workspace_id: &str,
        open: &[EditorFile],
        result: Value,
    ) -> Result<Value> {
        let mut workspace = model::workspace(&self.store, workspace_id)?;
        let mut layout_changed = false;
        let mut closing = Vec::new();
        let mut events = Vec::new();
        for pane_id in open.iter().filter_map(|file| file.pane_id.as_deref()) {
            let Some(running) = self.panes.get(pane_id).filter(|pane| !pane.stored.closed) else {
                continue;
            };
            let mut saved = running.stored.clone();
            saved.closed = true;
            if let Some(layout) = crate::workspace_layout::removing_pane(&workspace.layout, pane_id)
            {
                workspace.layout = layout;
                layout_changed = true;
            }
            events.push(model::pane_closed(pane_id, workspace_id)?);
            closing.push(saved);
        }
        if layout_changed {
            events.push((
                event::WORKSPACE_LAYOUT_UPDATED.into(),
                json!({"workspace":workspace}),
            ));
        }
        let mutation = self.mutate(connection, method::PROJECT_ENTRY_DELETE, params, |change| {
            for saved in &closing {
                persist_pane(change, saved)?;
            }
            if layout_changed {
                change.update_workspace(&workspace)?;
            }
            events.push(super::tabs::refresh_tabs(change, workspace_id)?);
            Ok((result, events))
        })?;
        if !mutation.repeated {
            for saved in closing {
                let id = saved.pane.pane_id.clone();
                if let Some(running) = self.panes.get_mut(&id) {
                    running.stored = saved;
                }
                self.finish_close(&id)?;
            }
        }
        Ok(mutation.result)
    }

    /// Writing documents keep sidecar state next to their file; they move and
    /// close through the writing surface, never under it.
    fn refuse_open_writing(&self, files: &[EditorFile]) -> Result<()> {
        let writing = files.iter().find(|file| {
            file.pane_id
                .as_deref()
                .and_then(|pane_id| self.panes.get(pane_id))
                .is_some_and(|pane| pane.stored.pane.mode == PaneMode::Writing)
        });
        match writing {
            Some(file) => Err(errors::invalid(format!(
                "{} is open as a writing document; close it first",
                file.relative_path
            ))),
            None => Ok(()),
        }
    }

    /// Editor files with a live pane at `path` or below it.
    fn live_files_under(&self, workspace_id: &str, path: &str) -> Result<Vec<EditorFile>> {
        let prefix = format!("{path}/");
        Ok(self
            .store
            .editor_files()
            .map_err(errors::storage)?
            .into_iter()
            .filter(|file| {
                file.workspace_id == workspace_id
                    && (file.relative_path == path || file.relative_path.starts_with(&prefix))
            })
            .collect())
    }
}

/// A pane named after its file follows the rename; a name the person chose stays.
fn renamed_pane(mut pane: model::StoredPane, old_path: &str, new_path: &str) -> model::StoredPane {
    let file_name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
    if pane.pane.name == file_name(old_path) {
        pane.pane.name = file_name(new_path);
        pane.pane.phrase = format!("Editing {}", pane.pane.name);
    }
    pane
}
