//! Fixtures shared by the unit tests of this crate.

use serde_json::{Value, json};
use soyeht_protocol::{Pane, Workspace};

pub fn workspace_json(id: &str) -> Value {
    json!({"workspaceId": id, "name": id, "path": "/work", "layout": {}})
}

pub fn pane_json(id: &str, workspace: &str, mode: &str) -> Value {
    json!({
        "paneId": id, "workspaceId": workspace, "name": id, "cwd": "/work", "mode": mode,
        "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
        "cols": 80, "rows": 24, "record": false
    })
}

pub fn workspace(id: &str) -> Workspace {
    serde_json::from_value(workspace_json(id)).unwrap()
}

pub fn pane(id: &str, workspace: &str, mode: &str) -> Pane {
    serde_json::from_value(pane_json(id, workspace, mode)).unwrap()
}
