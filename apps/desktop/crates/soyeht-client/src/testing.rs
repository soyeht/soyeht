//! Fixtures shared by the unit tests of this crate.

use serde_json::{Value, json};
use soyeht_protocol::{AgentSession, Pane, Workspace};

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

/// The listing of the folder `path`, as `project.tree.list` answers: each entry is a name and
/// whether it is a folder.
pub fn listing(path: &str, entries: &[(&str, bool)]) -> soyeht_protocol::ProjectTreeListResult {
    serde_json::from_value(json!({
        "workspaceId": "w", "path": path, "truncated": false,
        "entries": entries.iter().map(|(name, folder)| json!({
            "name": name,
            "path": if path.is_empty() { (*name).to_owned() } else { format!("{path}/{name}") },
            "kind": if *folder { "directory" } else { "file" },
            "ignored": false
        })).collect::<Vec<_>>()
    }))
    .unwrap()
}

/// A file of `contracts/vectors`: the cases the Mac client and this one both pass.
pub fn vector(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../contracts/vectors")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The cases listed under `key` in a file of `contracts/vectors`.
pub fn cases(file: &str, key: &str) -> Vec<Value> {
    vector(file)[key].as_array().unwrap().clone()
}

/// The text of a JSON string.
pub fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}

/// A notification the engine sent, with the fields every event carries.
pub fn event(name: soyeht_protocol::EventName, mut params: Value) -> crate::Event {
    params["eventId"] = json!("9");
    params["at"] = json!("now");
    crate::Event::Notification { name, params }
}

/// An agent session carrying `fields` besides the ones every session has.
pub fn agent_session(fields: Value) -> AgentSession {
    let mut session = json!({
        "harness": "claude", "version": "1", "fingerprint": "f", "options": [],
        "authMethods": [], "queue": [], "draft": false, "role": "worker",
    });
    for (key, field) in fields.as_object().into_iter().flatten() {
        session[key] = field.clone();
    }
    serde_json::from_value(session).unwrap()
}

/// An agent pane whose session carries `fields`.
pub fn agent_pane(id: &str, workspace: &str, fields: Value) -> Pane {
    let mut value = pane_json(id, workspace, "agent");
    value["agent"] = serde_json::to_value(agent_session(fields)).unwrap();
    serde_json::from_value(value).unwrap()
}
