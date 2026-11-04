//! What the web tests share: a JSON-RPC request, a dispatched reply, the next JSON frame
//! of a connection's outbox, a temporary workspace root, and a workspace with a web pane.

use crate::{core::Core, dispatch::Outcome, outbox};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(super) fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

pub(super) fn reply(core: &mut Core, connection: &str, request: Value) -> Value {
    let Outcome::Reply(value) = core.dispatch(connection, request).unwrap() else {
        panic!("expected reply")
    };
    value
}

/// A local human connection to `core`, as the server makes one: the challenge is read and `hello` sent
/// with `capabilities`. A web host's welcome is queued under the lock that makes it the host and is
/// the first thing its connection receives; any other is the reply.
pub(super) async fn connect_local(
    core: &mut Core,
    client: &str,
    capabilities: Value,
) -> (String, outbox::Receiver) {
    let (sender, mut receiver) = outbox::channel();
    let connection = core.connect(true, false, sender).unwrap();
    assert_eq!(next_json(&mut receiver).await["method"], "auth.challenge");
    let hello = request(
        1,
        "hello",
        json!({"client":client,"clientVersion":"1","protocolVersion":1,"capabilities":capabilities}),
    );
    let welcome = match core.dispatch(&connection, hello).unwrap() {
        Outcome::Reply(value) => value,
        Outcome::Queued => next_json(&mut receiver).await["result"].clone(),
        _ => panic!("expected a welcome"),
    };
    assert_eq!(welcome["role"], "human_local");
    (connection, receiver)
}

pub(super) async fn next_json(receiver: &mut outbox::Receiver) -> Value {
    let Frame::Json(bytes) = receiver.next().await.unwrap().unwrap().frame else {
        panic!("expected JSON frame")
    };
    serde_json::from_slice(&bytes).unwrap()
}

pub(super) fn temp_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("soyeht-web-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}

/// Creates the workspace "Web" on `root` as `connection` and returns its id.
pub(super) fn create_workspace(core: &mut Core, connection: &str, root: &Path) -> String {
    reply(
        core,
        connection,
        request(
            2,
            "workspace.create",
            json!({"name":"Web","path":root,"idempotencyKey":"workspace"}),
        ),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Creates a workspace on `root` and a web pane in it, as `connection`, and returns the pane id.
pub(super) fn create_web_pane(core: &mut Core, connection: &str, root: &Path) -> String {
    let workspace = create_workspace(core, connection, root);
    reply(
        core,
        connection,
        request(
            3,
            "pane.create",
            json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"pane"}),
        ),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned()
}
