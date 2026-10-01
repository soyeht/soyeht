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
    use crate::{core::Core, dispatch::Outcome, outbox, ports::testing::FixedClock};
    use serde_json::{Value, json};
    use soyeht_protocol::frame::Frame;
    use std::{fs, path::PathBuf, sync::Arc};
    use uuid::Uuid;

    struct Fixture {
        core: Core,
        connection: String,
        _receiver: outbox::Receiver,
        root: PathBuf,
        file_id: String,
        workspace_id: String,
        next_id: u64,
    }

    impl Fixture {
        async fn open(text: &str) -> Self {
            let root = std::env::temp_dir().join(format!("soyeht-editor-undo-{}", Uuid::new_v4()));
            fs::create_dir_all(root.join("project")).unwrap();
            fs::write(root.join("project/main.rs"), text).unwrap();
            let mut core = Core::open(&root.join("engine")).unwrap();
            core.clock = Arc::new(FixedClock(1_000));
            let (sender, mut receiver) = outbox::channel();
            let connection = core.connect(true, false, sender).unwrap();
            let Frame::Json(_) = receiver.next().await.unwrap().unwrap().frame else {
                panic!("expected the auth challenge")
            };
            let mut fixture = Self {
                core,
                connection,
                _receiver: receiver,
                root,
                file_id: String::new(),
                workspace_id: String::new(),
                next_id: 1,
            };
            fixture.call(
                "hello",
                json!({"client":"undo-test","clientVersion":"1","protocolVersion":1,"capabilities":[]}),
            );
            let project = fixture.root.join("project");
            fixture.workspace_id = fixture.call(
                "workspace.create",
                json!({"name":"Undo","path":project,"idempotencyKey":"workspace"}),
            )["workspace"]["workspaceId"]
                .as_str()
                .unwrap()
                .to_owned();
            let workspace_id = fixture.workspace_id.clone();
            fixture.file_id = fixture.call(
                "file.open",
                json!({"workspaceId":workspace_id,"path":"main.rs","idempotencyKey":"open"}),
            )["file"]["fileId"]
                .as_str()
                .unwrap()
                .to_owned();
            fixture
        }

        fn call(&mut self, method: &str, params: Value) -> Value {
            self.next_id += 1;
            let request =
                json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params});
            let Outcome::Reply(value) = self.core.dispatch(&self.connection, request).unwrap()
            else {
                panic!("expected a reply to {method}")
            };
            value
        }

        fn read(&mut self) -> Value {
            let file_id = self.file_id.clone();
            self.call(
                "file.read",
                json!({"fileId":file_id,"fromLine":0,"lineCount":20}),
            )
        }

        fn revision(&mut self) -> String {
            self.read()["file"]["revision"].as_str().unwrap().to_owned()
        }

        fn text(&mut self) -> String {
            self.read()["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line["text"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn type_at(&mut self, now_ms: i64, line: u32, column: u32, text: &str) {
            self.core.clock = Arc::new(FixedClock(now_ms));
            let revision = self.revision();
            let file_id = self.file_id.clone();
            let position = json!({"line":line,"column":column});
            self.call(
                "file.write",
                json!({
                    "fileId":file_id,"baseRevision":revision,
                    "author":{"actor":{"role":"human_local","client":"undo-test"},"name":"you"},
                    "edits":[{"range":{"start":position,"end":position},"text":text}],
                    "idempotencyKey":Uuid::new_v4().to_string()
                }),
            );
        }

        fn replay(&mut self, method: &str, key: &str) -> Value {
            let revision = self.revision();
            let file_id = self.file_id.clone();
            self.call(
                method,
                json!({"fileId":file_id,"baseRevision":revision,"idempotencyKey":key}),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

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
    async fn stale_revisions_fail_and_disk_reloads_drop_the_history() {
        let mut fixture = Fixture::open("alpha\n").await;
        fixture.type_at(1_000, 0, 5, " beta");
        let file_id = fixture.file_id.clone();
        let stale = fixture.core.dispatch(
            &fixture.connection,
            json!({"jsonrpc":"2.0","id":99,"method":"file.undo",
                "params":{"fileId":file_id,"baseRevision":"1","idempotencyKey":"stale"}}),
        );
        assert!(stale.is_err());

        // Autosave, then an agent rewrites the file on disk: the reload moves the
        // revision without a recorded step, so the old positions must not replay.
        fixture.core.clock = Arc::new(FixedClock(10_000));
        fixture.core.advance_editor().unwrap();
        assert_eq!(fixture.read()["file"]["dirty"], false);
        let path = fixture.root.join("project/main.rs");
        assert_eq!(fs::read_to_string(&path).unwrap(), "alpha beta\n");
        fs::write(&path, "rewritten by an agent\n").unwrap();
        fixture.core.clock = Arc::new(FixedClock(20_000));
        fixture.core.advance_editor().unwrap();
        assert_eq!(fixture.text(), "rewritten by an agent\n");
        let undone = fixture.replay("file.undo", "after-reload");
        assert_eq!(undone["applied"], false);
        assert_eq!(fixture.text(), "rewritten by an agent\n");
    }
}
