//! Engine-owned editor tabs: `editor.tabs.list`, `editor.tabs.move`, and the
//! tab bookkeeping of `file.open` and `pane.close`.
//!
//! A tab is a live editor pane; ADR 0046 still gives each file exactly one
//! owner pane. The engine keeps their order and the workspace's single preview
//! tab, so every paired client shows the same strip. A preview stays a preview
//! while its file keeps the revision it was previewed at, which makes the first
//! edit pin it without the write path touching tabs (ADR 0072).

use super::persistence::parameters;
use crate::{
    core::{Core, persist_pane},
    errors::{self, Result},
    model::{RunningPane, StoredPane},
    panes::persist_new_pane,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    EditorTab, EditorTabsListParams, EditorTabsListResult, EditorTabsMoveParams, Workspace,
};
use soyeht_store::{
    database::Change,
    editor::{EditorFile, StoredFileVersion},
    editor_tabs::{EditorTabPlacement, EditorTabRow},
};

type StoreResult<T> = soyeht_store::database::Result<T>;

/// A new editor pane that `file.open` or `pane.create` is about to commit.
pub(crate) struct EditorPaneOpening<'a> {
    pub connection: &'a str,
    pub method: &'a str,
    pub params: &'a Value,
    pub saved: StoredPane,
    pub file: EditorFile,
    pub syntax: soyeht_editor::SyntaxDocument,
    pub reply: Value,
    pub created_event: Value,
    pub version: Option<StoredFileVersion>,
    pub preview: bool,
}

/// The previous preview tab that a new preview replaces.
struct ReplacedPreview {
    saved: StoredPane,
    index: usize,
    layout: Option<Workspace>,
    events: Vec<(String, Value)>,
}

impl Core {
    pub fn editor_tabs_list(&self, params: &Value) -> Result<Value> {
        let request: EditorTabsListParams = parameters(params, "tabs list")?;
        crate::model::workspace(&self.store, &request.workspace_id)?;
        let rows = self
            .store
            .editor_tabs(&request.workspace_id)
            .map_err(errors::storage)?;
        tabs_result(&request.workspace_id, &rows)
    }

    pub fn editor_tabs_move(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: EditorTabsMoveParams = parameters(params, "tabs move")?;
        let rows = self
            .store
            .editor_tabs(&request.workspace_id)
            .map_err(errors::storage)?;
        let mut order = placements(&rows);
        let from = order
            .iter()
            .position(|placement| placement.pane_id == request.pane_id)
            .ok_or_else(|| errors::not_found("Editor tab not found"))?;
        let moved = order.remove(from);
        order.insert((request.index as usize).min(order.len()), moved);
        let workspace_id = request.workspace_id.clone();
        Ok(self
            .mutate(connection, "editor.tabs.move", params, |change| {
                change.replace_editor_tabs(&workspace_id, &order)?;
                let rows = change.editor_tabs(&workspace_id)?;
                let result = tabs_value(&workspace_id, &rows)?;
                Ok((result.clone(), vec![("editor.tabs_updated".into(), result)]))
            })?
            .result)
    }

    /// Commits a new editor pane as a tab. A preview replaces the workspace's
    /// current preview when that one is still unedited, taking its position.
    pub(crate) fn commit_editor_pane(&mut self, opening: EditorPaneOpening<'_>) -> Result<Value> {
        let workspace_id = opening.saved.pane.workspace_id.clone();
        let pane_id = opening.saved.pane.pane_id.clone();
        let replaced = if opening.preview {
            self.replaceable_preview(&workspace_id, &opening.file.relative_path)?
        } else {
            None
        };
        let screen = Screen::open(&self.directory.join("sessions").join(&pane_id), false)?;
        let preview_revision = opening.preview.then_some(opening.file.revision);
        let mutation = self.mutate(
            opening.connection,
            opening.method,
            opening.params,
            |change| {
                persist_new_pane(change, &opening.saved, None)?;
                change.save_editor_file(&opening.file)?;
                if let Some(version) = &opening.version {
                    change.record_file_version(version)?;
                }
                let mut events = vec![("pane.created".into(), opening.created_event.clone())];
                if let Some(replaced) = &replaced {
                    persist_pane(change, &replaced.saved)?;
                    if let Some(workspace) = &replaced.layout {
                        change.update_workspace(workspace)?;
                    }
                    events.extend(replaced.events.iter().cloned());
                }
                let index = replaced.as_ref().map(|replaced| replaced.index);
                events.push(place_tab(
                    change,
                    &workspace_id,
                    &pane_id,
                    preview_revision,
                    index,
                )?);
                Ok((opening.reply.clone(), events))
            },
        )?;
        if !mutation.repeated {
            self.panes.insert(
                pane_id,
                RunningPane {
                    stored: opening.saved,
                    screen,
                },
            );
            self.editor.insert_parsed(opening.file, opening.syntax);
            if let Some(replaced) = replaced {
                let id = replaced.saved.pane.pane_id.clone();
                errors::invariant(self.panes.get_mut(&id), "Preview pane disappeared")?.stored =
                    replaced.saved;
                self.finish_close(&id)?;
            }
        }
        Ok(mutation.result)
    }

    /// Saves a pending draft before its editor pane closes, so closing a tab
    /// keeps the autosave promise. A conflicted draft stays in the database
    /// and comes back when the file is opened again.
    pub(crate) fn settle_editor_pane_before_close(&mut self, pane_id: &str) {
        let Ok(Some(file)) = self.store.editor_file_for_pane(pane_id) else {
            return;
        };
        if let Err(error) = self.save_editor_now(&file.file_id) {
            eprintln!(
                "{}",
                json!({"event":"editor.close_save_failed","fileId":file.file_id,"reason":error.to_string()})
            );
        }
    }

    fn replaceable_preview(
        &self,
        workspace_id: &str,
        opening_path: &str,
    ) -> Result<Option<ReplacedPreview>> {
        let rows = self
            .store
            .editor_tabs(workspace_id)
            .map_err(errors::storage)?;
        let Some((index, row)) = rows.iter().enumerate().find(|(_, row)| {
            row.is_preview()
                && row.relative_path != opening_path
                && self
                    .editor
                    .stored(&row.file_id)
                    .is_ok_and(|file| file.draft_text.is_none())
        }) else {
            return Ok(None);
        };
        let Some(running) = self.panes.get(&row.pane_id) else {
            return Ok(None);
        };
        let mut saved = running.stored.clone();
        let mut events = Vec::new();
        let layout = self.prepare_pane_close(&mut saved, &mut events)?;
        Ok(Some(ReplacedPreview {
            saved,
            index,
            layout,
            events,
        }))
    }
}

/// The tab event after a pane change in this transaction, keeping the order.
pub(crate) fn refresh_tabs(
    change: &Change<'_>,
    workspace_id: &str,
) -> StoreResult<(String, Value)> {
    let rows = change.editor_tabs(workspace_id)?;
    change.replace_editor_tabs(workspace_id, &placements(&rows))?;
    Ok((
        "editor.tabs_updated".into(),
        tabs_value(workspace_id, &rows)?,
    ))
}

/// Pins a preview tab, as opening its file for keeps or double-clicking does.
pub(crate) fn pin_tab(
    change: &Change<'_>,
    workspace_id: &str,
    pane_id: &str,
) -> StoreResult<Option<(String, Value)>> {
    let rows = change.editor_tabs(workspace_id)?;
    if !rows
        .iter()
        .any(|row| row.pane_id == pane_id && row.is_preview())
    {
        return Ok(None);
    }
    let mut order = placements(&rows);
    for placement in &mut order {
        if placement.pane_id == pane_id {
            placement.preview_revision = None;
        }
    }
    change.replace_editor_tabs(workspace_id, &order)?;
    let rows = change.editor_tabs(workspace_id)?;
    Ok(Some((
        "editor.tabs_updated".into(),
        tabs_value(workspace_id, &rows)?,
    )))
}

fn place_tab(
    change: &Change<'_>,
    workspace_id: &str,
    pane_id: &str,
    preview_revision: Option<u64>,
    index: Option<usize>,
) -> StoreResult<(String, Value)> {
    let rows = change.editor_tabs(workspace_id)?;
    let mut order = placements(&rows);
    order.retain(|placement| placement.pane_id != pane_id);
    if preview_revision.is_some() {
        // A workspace keeps at most one preview tab.
        for placement in &mut order {
            placement.preview_revision = None;
        }
    }
    let placement = EditorTabPlacement {
        pane_id: pane_id.into(),
        preview_revision,
    };
    match index {
        Some(index) => order.insert(index.min(order.len()), placement),
        None => order.push(placement),
    }
    change.replace_editor_tabs(workspace_id, &order)?;
    let rows = change.editor_tabs(workspace_id)?;
    Ok((
        "editor.tabs_updated".into(),
        tabs_value(workspace_id, &rows)?,
    ))
}

/// Current order with stale previews (edited since) made permanent.
fn placements(rows: &[EditorTabRow]) -> Vec<EditorTabPlacement> {
    rows.iter()
        .map(|row| EditorTabPlacement {
            pane_id: row.pane_id.clone(),
            preview_revision: row.is_preview().then_some(row.file_revision),
        })
        .collect()
}

fn wire(rows: &[EditorTabRow]) -> Vec<EditorTab> {
    rows.iter()
        .map(|row| EditorTab {
            pane_id: row.pane_id.clone(),
            file_id: row.file_id.clone(),
            path: row.relative_path.clone(),
            preview: row.is_preview(),
            preview_revision: row.is_preview().then(|| row.file_revision.to_string()),
        })
        .collect()
}

fn tabs_value(workspace_id: &str, rows: &[EditorTabRow]) -> StoreResult<Value> {
    Ok(serde_json::to_value(EditorTabsListResult {
        workspace_id: workspace_id.into(),
        tabs: wire(rows),
    })?)
}

fn tabs_result(workspace_id: &str, rows: &[EditorTabRow]) -> Result<Value> {
    tabs_value(workspace_id, rows).map_err(errors::storage)
}
