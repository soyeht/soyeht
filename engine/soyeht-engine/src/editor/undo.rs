//! `file.undo` and `file.redo`: replay the engine-owned edit history of one file.
//!
//! The history lives with the open document, not in a client view, so every
//! paired client (Mac, iPad, iPhone) undoes the same edits, including edits
//! outside its loaded viewport. Replays go through the same durable write path
//! as `file.write`, so autosave, version history and diagnostics follow them.

use super::persistence::{parameters, revision};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::Value;
use soyeht_editor::HistoryDirection;
use soyeht_protocol::{FileUndoParams, FileUndoResult};
use soyeht_store::editor::document;

impl Core {
    pub fn file_undo(&mut self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        let request: FileUndoParams = parameters(params, "undo")?;
        let direction = match method {
            "file.undo" => HistoryDirection::Undo,
            "file.redo" => HistoryDirection::Redo,
            _ => return Err(errors::internal("Unknown file history method")),
        };
        let author = self.file_author(connection)?;
        let prepared = self.editor.prepare_replay(
            &request.file_id,
            revision(&request.base_revision)?,
            direction,
            author,
            self.now_ms(),
        )?;
        let Some(prepared) = prepared else {
            let file = self.editor.stored(&request.file_id)?.clone();
            let result = undo_result(FileUndoResult {
                file: document(file).map_err(errors::storage)?,
                applied: false,
                changed_lines: Vec::new(),
                cursor: None,
            })?;
            return Ok(self
                .mutate(connection, method, params, |_| Ok((result, Vec::new())))?
                .result);
        };
        let result = undo_result(FileUndoResult {
            file: document(prepared.file.clone()).map_err(errors::storage)?,
            applied: true,
            changed_lines: prepared.changed_lines.clone(),
            cursor: prepared.cursor.clone(),
        })?;
        self.commit_prepared_edit(connection, method, params, prepared, result)
    }
}

fn undo_result(result: FileUndoResult) -> Result<Value> {
    serde_json::to_value(result)
        .map_err(|_| errors::internal("File undo result serialization failed"))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::Fixture;
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn undo_and_redo_replay_typing_bursts_through_the_durable_write_path() {
        let mut fixture = Fixture::open("fn main() {}\n").await;
        fixture.type_at(1_000, 0, 11, "l");
        fixture.type_at(1_100, 0, 12, "e");
        fixture.type_at(1_200, 0, 13, "t");
        fixture.type_at(5_000, 0, 14, ";");
        assert_eq!(fixture.text(), "fn main() {let;}\n");

        let undone = fixture.replay("file.undo", "undo-1");
        assert_eq!(undone["applied"], true);
        assert_eq!(undone["file"]["dirty"], true);
        assert_eq!(undone["cursor"], json!({"line":0,"column":14}));
        assert_eq!(fixture.text(), "fn main() {let}\n");
        let file_id = fixture.file_id.clone();
        let repeated = fixture.call(
            "file.undo",
            json!({"fileId":file_id,"baseRevision":"5","idempotencyKey":"undo-1"}),
        );
        assert_eq!(repeated, undone);
        assert_eq!(fixture.text(), "fn main() {let}\n");

        let burst = fixture.replay("file.undo", "undo-2");
        assert_eq!(burst["cursor"], json!({"line":0,"column":11}));
        assert_eq!(fixture.text(), "fn main() {}\n");
        let empty = fixture.replay("file.undo", "undo-3");
        assert_eq!(empty["applied"], false);
        assert_eq!(fixture.text(), "fn main() {}\n");

        let redone = fixture.replay("file.redo", "redo-1");
        assert_eq!(redone["applied"], true);
        assert_eq!(redone["cursor"], json!({"line":0,"column":14}));
        assert_eq!(fixture.text(), "fn main() {let}\n");
        fixture.type_at(9_000, 0, 0, "// ");
        assert_eq!(fixture.replay("file.redo", "redo-2")["applied"], false);
        assert_eq!(fixture.text(), "// fn main() {let}\n");
    }

    #[tokio::test]
    async fn undo_lands_the_cursor_inside_the_restored_line() {
        // Typing grew line 2 to 17 characters; after undo it is 9 again and
        // the cursor must point at the restored edit, not past the line end.
        let mut fixture = Fixture::open("fn main() {\nlet x = 1\n}\n").await;
        fixture.type_at(1_000, 1, 9, " + 2 + 3");
        let undone = fixture.replay("file.undo", "undo-line");
        assert_eq!(undone["cursor"], json!({"line":1,"column":9}));
        assert_eq!(fixture.text(), "fn main() {\nlet x = 1\n}\n");
        let redone = fixture.replay("file.redo", "redo-line");
        assert_eq!(redone["cursor"], json!({"line":1,"column":17}));
    }

    #[tokio::test]
    async fn stale_revisions_fail_and_disk_reloads_drop_the_history() {
        let mut fixture = Fixture::open("alpha\n").await;
        fixture.type_at(1_000, 0, 5, " beta");
        let file_id = fixture.file_id.clone();
        let stale = fixture.try_call(
            "file.undo",
            json!({"fileId":file_id,"baseRevision":"1","idempotencyKey":"stale"}),
        );
        assert!(stale.is_err());

        // Autosave, then an agent rewrites the file on disk: the reload moves the
        // revision without a recorded step, so the old positions must not replay.
        fixture.at(10_000);
        fixture.core.advance_editor().unwrap();
        assert_eq!(fixture.read()["file"]["dirty"], false);
        let path = fixture.root.join("project/main.rs");
        assert_eq!(fs::read_to_string(&path).unwrap(), "alpha beta\n");
        fs::write(&path, "rewritten by an agent\n").unwrap();
        fixture.at(20_000);
        fixture.core.advance_editor().unwrap();
        assert_eq!(fixture.text(), "rewritten by an agent\n");
        let undone = fixture.replay("file.undo", "after-reload");
        assert_eq!(undone["applied"], false);
        assert_eq!(fixture.text(), "rewritten by an agent\n");
    }
}
