//! Editor tab order and the project path moves that keep editor files attached.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{Connection, params};

/// One live editor pane of a workspace, in tab order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorTabRow {
    pub pane_id: String,
    pub file_id: String,
    pub relative_path: String,
    pub file_revision: u64,
    pub preview: bool,
    pub preview_revision: Option<u64>,
}

impl EditorTabRow {
    /// A preview tab stays a preview only while its file keeps the revision it
    /// had when it was previewed; the first edit makes it permanent.
    pub fn is_preview(&self) -> bool {
        self.preview && self.preview_revision == Some(self.file_revision)
    }
}

/// The stored position of one tab: it is a preview when it carries the file
/// revision it was previewed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorTabPlacement {
    pub pane_id: String,
    pub preview_revision: Option<u64>,
}

impl Store {
    pub fn editor_tabs(&self, workspace_id: &str) -> Result<Vec<EditorTabRow>> {
        tab_rows(&self.connection, workspace_id)
    }
}

impl Change<'_> {
    /// Tabs as this transaction sees them, including panes it created or closed.
    pub fn editor_tabs(&self, workspace_id: &str) -> Result<Vec<EditorTabRow>> {
        tab_rows(self.connection(), workspace_id)
    }

    /// Replaces the whole tab order of a workspace.
    pub fn replace_editor_tabs(
        &self,
        workspace_id: &str,
        order: &[EditorTabPlacement],
    ) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM editor_tabs WHERE workspace_id=?1",
            [workspace_id],
        )?;
        for (ordinal, placement) in order.iter().enumerate() {
            let preview_revision = placement
                .preview_revision
                .map(|value| i64::try_from(value).map_err(|_| Error::InvalidEditorFile))
                .transpose()?;
            let ordinal = i64::try_from(ordinal).map_err(|_| Error::InvalidEditorFile)?;
            self.transaction.execute(
                "INSERT INTO editor_tabs(pane_id,workspace_id,ordinal,preview,preview_revision) VALUES (?1,?2,?3,?4,?5)",
                params![
                    placement.pane_id,
                    workspace_id,
                    ordinal,
                    preview_revision.is_some(),
                    preview_revision
                ],
            )?;
        }
        Ok(())
    }

    /// Moves every editor file at `from` or below it to `to`, so open buffers,
    /// drafts and local history follow a rename. A closed file that already
    /// claimed a path under `to` belongs to an entry that no longer exists on
    /// disk; it is dropped. A live one makes the move fail.
    pub fn move_editor_paths(&self, workspace_id: &str, from: &str, to: &str) -> Result<usize> {
        self.transaction.execute(
            "DELETE FROM editor_files WHERE workspace_id=?1 AND (relative_path=?2 OR substr(relative_path,1,length(?2)+1)=?2||'/') AND pane_id IN (SELECT id FROM panes WHERE closed=1)",
            params![workspace_id, to],
        )?;
        let moved = self.transaction.execute(
            "UPDATE editor_files SET relative_path=?3||substr(relative_path,length(?2)+1) WHERE workspace_id=?1 AND (relative_path=?2 OR substr(relative_path,1,length(?2)+1)=?2||'/')",
            params![workspace_id, from, to],
        )?;
        Ok(moved)
    }
}

fn tab_rows(connection: &Connection, workspace_id: &str) -> Result<Vec<EditorTabRow>> {
    let mut statement = connection.prepare(
        "SELECT p.id,e.file_id,e.relative_path,e.revision,t.preview,t.preview_revision FROM panes p JOIN editor_files e ON e.pane_id=p.id LEFT JOIN editor_tabs t ON t.pane_id=p.id WHERE p.workspace_id=?1 AND p.closed=0 AND json_extract(p.payload,'$.mode')='editor' ORDER BY t.ordinal IS NULL,t.ordinal,p.created_seq,p.rowid",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<bool>>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    })?;
    rows.map(|row| {
        let (pane_id, file_id, relative_path, revision, preview, preview_revision) = row?;
        Ok(EditorTabRow {
            pane_id,
            file_id,
            relative_path,
            file_revision: u64::try_from(revision).map_err(|_| Error::InvalidEditorFile)?,
            preview: preview.unwrap_or(false),
            preview_revision: preview_revision
                .map(|value| u64::try_from(value).map_err(|_| Error::InvalidEditorFile))
                .transpose()?,
        })
    })
    .collect()
}
