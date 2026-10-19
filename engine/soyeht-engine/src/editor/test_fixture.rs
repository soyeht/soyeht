//! A real engine core over a temporary project, driven through `dispatch`
//! by one signed-in local connection, for editor tests.

use super::{FileSystemPort, Service, SystemFiles};
use crate::{core::Core, dispatch::Outcome, hooks::Hooks, outbox, ports::testing::FixedClock};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

/// Holds the editor's autosaves while a test asks it to.
#[derive(Default)]
struct AutosaveHold(AtomicBool);

impl Hooks for AutosaveHold {
    fn pause_editor_autosave(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub(crate) struct EditorFixture {
    pub core: Core,
    autosave: Arc<AutosaveHold>,
    pub connection: String,
    pub receiver: outbox::Receiver,
    pub root: PathBuf,
    pub workspace_id: String,
    next_id: u64,
}

impl EditorFixture {
    /// A workspace at `<temp>/project` holding `files`.
    pub async fn new(files: &[(&str, &str)]) -> Self {
        Self::over(files, Box::new(SystemFiles)).await
    }

    /// The same, with the editor reaching the disk through `port`.
    pub async fn over(files: &[(&str, &str)], port: Box<dyn FileSystemPort>) -> Self {
        let root = std::env::temp_dir().join(format!("soyeht-editor-{}", Uuid::new_v4()));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        for (path, text) in files {
            let path = project.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let autosave = Arc::new(AutosaveHold::default());
        let mut core = Core::open_with(&root.join("engine"), autosave.clone()).unwrap();
        core.clock = Arc::new(FixedClock(1_000));
        core.editor = Service::new(port);
        // The disk jobs of the tick run and apply in the caller, for determinism.
        core.background = crate::offload::Background::inline();
        let (sender, mut receiver) = outbox::channel();
        let connection = core.connect(true, false, sender).unwrap();
        let Frame::Json(_) = receiver.next().await.unwrap().unwrap().frame else {
            panic!("expected the auth challenge")
        };
        let mut fixture = Self {
            core,
            autosave,
            connection,
            receiver,
            root,
            workspace_id: String::new(),
            next_id: 1,
        };
        fixture.call(
            "hello",
            json!({"client":"editor-test","clientVersion":"1","protocolVersion":1,"capabilities":[]}),
        );
        fixture.workspace_id = fixture.call(
            "workspace.create",
            json!({"name":"Editor","path":project,"idempotencyKey":"workspace"}),
        )["workspace"]["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        fixture
    }

    pub fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    /// Holds (or releases) the editor's autosaves, so a draft stays a draft.
    pub fn pause_autosave(&self, paused: bool) {
        self.autosave.0.store(paused, Ordering::Relaxed);
    }

    pub fn at(&mut self, now_ms: i64) {
        self.core.clock = Arc::new(FixedClock(now_ms));
    }

    pub fn try_call(&mut self, method: &str, params: Value) -> crate::errors::Result<Value> {
        self.next_id += 1;
        let request = json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params});
        match self.core.dispatch_inline(&self.connection, request)? {
            Outcome::Reply(value) => Ok(value),
            _ => panic!("expected a reply to {method}"),
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        self.try_call(method, params).unwrap()
    }

    /// Opens `path` and returns the `file.open` result.
    pub fn open(&mut self, path: &str, preview: bool) -> Value {
        let workspace_id = self.workspace_id.clone();
        self.call(
            "file.open",
            json!({"workspaceId":workspace_id,"path":path,"preview":preview,
                "idempotencyKey":Uuid::new_v4().to_string()}),
        )
    }

    pub fn read(&mut self, file_id: &str) -> Value {
        self.call(
            "file.read",
            json!({"fileId":file_id,"fromLine":0,"lineCount":20}),
        )
    }

    pub fn text(&mut self, file_id: &str) -> String {
        self.read(file_id)["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Types `text` at a position, as the person at this connection.
    pub fn type_at(&mut self, file_id: &str, line: u32, column: u32, text: &str) -> Value {
        let revision = self.read(file_id)["file"]["revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let position = json!({"line":line,"column":column});
        self.call(
            "file.write",
            json!({
                "fileId":file_id,"baseRevision":revision,
                "author":{"actor":{"role":"human_local","client":"editor-test"},"name":"you"},
                "edits":[{"range":{"start":position,"end":position},"text":text}],
                "idempotencyKey":Uuid::new_v4().to_string()
            }),
        )
    }

    /// Notification frames already queued for this connection.
    pub async fn notifications(&mut self) -> Vec<Value> {
        let mut found = Vec::new();
        while let Ok(Ok(Some(packet))) =
            tokio::time::timeout(Duration::from_millis(20), self.receiver.next()).await
        {
            if let Frame::Json(bytes) = packet.frame {
                found.push(serde_json::from_slice(&bytes).unwrap());
            }
        }
        found
    }
}

impl Drop for EditorFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
