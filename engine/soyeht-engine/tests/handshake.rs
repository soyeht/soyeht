//! Refused handshakes: what a client that reconnects can learn from the answer.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::os::unix::net::UnixStream;

mod support;
use support::*;

/// Sends a `hello` with `params` over a fresh local connection and returns the answer.
fn hello_with(rig: &Rig, params: Value) -> Value {
    let mut socket = UnixStream::connect(rig.path.join("engine.sock")).unwrap();
    socket
        .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
        .unwrap();
    let Some(Frame::Json(challenge)) = Frame::read_from(&mut socket).unwrap() else {
        panic!("challenge")
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&challenge).unwrap()["method"],
        "auth.challenge"
    );
    let request = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":params});
    Frame::Json(serde_json::to_vec(&request).unwrap())
        .write_to(&mut socket)
        .unwrap();
    let Some(Frame::Json(answer)) = Frame::read_from(&mut socket).unwrap() else {
        panic!("hello answer")
    };
    serde_json::from_slice(&answer).unwrap()
}

fn hello(rig: &Rig, capabilities: Value) -> Value {
    hello_with(
        rig,
        json!({"client":"second-app","clientVersion":"1","protocolVersion":1,
            "capabilities":capabilities}),
    )
}

#[test]
fn a_second_web_host_is_refused_as_unavailable_not_as_unauthorized() {
    let rig = Rig::new();
    let _host = Client::new_with_capabilities(rig.path.join("engine.sock"), json!(["web_host"]));

    let refused = hello(&rig, json!(["web_host"]));
    assert_eq!(refused["error"]["data"]["kind"], "web_host_unavailable");

    // The same client is welcome as an ordinary Mac connection.
    let welcome = hello(&rig, json!([]));
    assert_eq!(welcome["result"]["role"], "human_local");
}

#[test]
fn an_unknown_agent_binding_is_unauthorized() {
    let rig = Rig::new();
    let answer = hello_with(
        &rig,
        json!({"client":"soyeht-mcp","clientVersion":"1","protocolVersion":1,
            "binding":"a-binding-the-engine-never-issued"}),
    );
    assert_eq!(answer["error"]["data"]["kind"], "unauthorized");
    assert_eq!(answer["error"]["message"], "Unknown agent binding");
}
