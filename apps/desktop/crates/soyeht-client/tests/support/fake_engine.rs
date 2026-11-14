//! The engine side of the client's tests: a scripted engine on a real local endpoint (a Unix socket,
//! or on Windows the loopback endpoint file), the handshake it answers and the helpers to reach it.
#![allow(dead_code)]

use serde_json::{Value, json};
use soyeht_client::{CLIENT_NAME, Event};
use soyeht_platform::local::{Listener, Stream};
use soyeht_protocol::frame::{read_json, write_json};
use std::{path::PathBuf, time::Duration};

/// The engine side of one connection.
pub struct FakeEngine(pub Stream);

impl FakeEngine {
    pub fn accept(listener: &Listener) -> Self {
        let stream = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut engine = Self(stream);
        engine.send(&json!({"jsonrpc": "2.0", "method": "auth.challenge",
            "params": {"nonce": "AAAA", "engineId": "e1"}}));
        engine
    }

    pub fn send(&mut self, value: &Value) {
        write_json(&mut self.0, value).unwrap();
    }

    pub fn request(&mut self) -> Value {
        read_json(&mut self.0)
            .unwrap()
            .expect("client closed early")
    }

    pub fn reply(&mut self, request: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": result}));
    }

    /// The engine's refusal of `request`, with the code and the kind the contract gives.
    pub fn refuse(&mut self, request: &Value, kind: &str, code: i64, message: &str) {
        self.send(&json!({"jsonrpc": "2.0", "id": request["id"], "error": {
            "code": code, "message": message, "data": {"kind": kind}}}));
    }

    pub fn expect(&mut self, method: &str) -> Value {
        let request = self.request();
        assert_eq!(request["method"], method);
        request
    }

    /// hello, state.snapshot (one workspace, one terminal) and events.subscribe.
    pub fn handshake(&mut self) {
        self.handshake_hello();
    }

    /// [`handshake`](Self::handshake), returning the client's `hello`.
    pub fn handshake_hello(&mut self) -> Value {
        self.handshake_offering(&[], &[])
    }

    /// The handshake of an engine that offers `capabilities` and whose snapshot has the sections
    /// of `more` after the workspace and the pane.
    pub fn handshake_offering(&mut self, capabilities: &[&str], more: &[(&str, Value)]) -> Value {
        let hello = self.expect("hello");
        assert_eq!(hello["params"]["client"], CLIENT_NAME);
        self.reply(
            &hello,
            json!({"engineVersion": "0", "protocolVersion": 1,
            "connectionId": "c1", "role": "human_local", "capabilities": capabilities, "dbId": "db"}),
        );
        let snapshot = self.expect("state.snapshot");
        let parts = 2 + more.len();
        self.send(&notification(
            "state.snapshot_part",
            json!({
            "snapshotId": "s1", "index": 0, "of": parts, "section": "workspaces",
            "data": [{"workspaceId": "w1", "name": "api", "path": "/work", "layout": {}}]}),
        ));
        self.send(&notification(
            "state.snapshot_part",
            json!({
            "snapshotId": "s1", "index": 1, "of": parts, "section": "panes",
            "data": [{"paneId": "t1", "workspaceId": "w1", "name": "shell", "cwd": "/work",
                "mode": "terminal", "state": "idle", "source": "none", "confidence": 0.0,
                "phrase": "", "generation": 1, "cols": 80, "rows": 24, "record": false}]}),
        ));
        for (index, (section, data)) in more.iter().enumerate() {
            self.send(&notification(
                "state.snapshot_part",
                json!({"snapshotId": "s1", "index": 2 + index, "of": parts,
                "section": section, "data": data}),
            ));
        }
        self.reply(
            &snapshot,
            json!({"snapshotId": "s1", "cursor": "7", "parts": parts,
            "focus": {"workspaceId": "w1", "paneId": "t1"}}),
        );
        let subscribe = self.expect("events.subscribe");
        assert_eq!(subscribe["params"]["since"], "7");
        self.reply(&subscribe, json!({"subscriptionId": "sub", "through": "7"}));
        hello
    }
}

pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

pub fn socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("soyeht-client-{name}-{}.sock", std::process::id()))
}

pub fn next(inbox: &async_channel::Receiver<Event>) -> Event {
    inbox.recv_blocking().unwrap()
}

pub fn listen(name: &str) -> (Listener, PathBuf) {
    let path = socket_path(name);
    let _ = std::fs::remove_file(&path);
    (Listener::bind(&path).unwrap(), path)
}

/// The next event that satisfies `wanted`, within ten seconds.
pub fn wait_for(inbox: &async_channel::Receiver<Event>, wanted: impl Fn(&Event) -> bool) -> Event {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match inbox.try_recv() {
            Ok(event) if wanted(&event) => return event,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("the event did not come");
}
