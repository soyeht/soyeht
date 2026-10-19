//! What the web tests share: a JSON-RPC request, a dispatched reply, the next JSON frame
//! of a connection's outbox, and a temporary workspace root.

use crate::{core::Core, dispatch::Outcome, outbox};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::{fs, path::PathBuf};
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
