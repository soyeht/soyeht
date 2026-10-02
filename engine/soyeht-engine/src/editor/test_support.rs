//! One open editor file behind a real engine core, for request-level tests.

use crate::{core::Core, dispatch::Outcome, outbox, ports::testing::FixedClock};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::{fs, path::PathBuf, sync::Arc};
use uuid::Uuid;

pub(super) struct Fixture {
    pub core: Core,
    pub connection: String,
    _receiver: outbox::Receiver,
    pub root: PathBuf,
    pub file_id: String,
    next_id: u64,
}

impl Fixture {
    pub async fn open(text: &str) -> Self {
        let root = std::env::temp_dir().join(format!("soyeht-editor-file-{}", Uuid::new_v4()));
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
            next_id: 1,
        };
        fixture.call(
            "hello",
            json!({"client":"editor-test","clientVersion":"1","protocolVersion":1,"capabilities":[]}),
        );
        let project = fixture.root.join("project");
        let workspace_id = fixture.call(
            "workspace.create",
            json!({"name":"Editor","path":project,"idempotencyKey":"workspace"}),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        fixture.file_id = fixture.call(
            "file.open",
            json!({"workspaceId":workspace_id,"path":"main.rs","idempotencyKey":"open"}),
        )["file"]["fileId"]
            .as_str()
            .unwrap()
            .to_owned();
        fixture
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let request = json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params});
        let Outcome::Reply(value) = self.core.dispatch(&self.connection, request).unwrap() else {
            panic!("expected a reply to {method}")
        };
        value
    }

    pub fn read(&mut self) -> Value {
        let file_id = self.file_id.clone();
        self.call(
            "file.read",
            json!({"fileId":file_id,"fromLine":0,"lineCount":20}),
        )
    }

    pub fn revision(&mut self) -> String {
        self.read()["file"]["revision"].as_str().unwrap().to_owned()
    }

    pub fn text(&mut self) -> String {
        self.read()["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn type_at(&mut self, now_ms: i64, line: u32, column: u32, text: &str) {
        self.core.clock = Arc::new(FixedClock(now_ms));
        let revision = self.revision();
        let file_id = self.file_id.clone();
        let position = json!({"line":line,"column":column});
        self.call(
            "file.write",
            json!({
                "fileId":file_id,"baseRevision":revision,
                "author":{"actor":{"role":"human_local","client":"editor-test"},"name":"you"},
                "edits":[{"range":{"start":position,"end":position},"text":text}],
                "idempotencyKey":Uuid::new_v4().to_string()
            }),
        );
    }

    pub fn replay(&mut self, method: &str, key: &str) -> Value {
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
