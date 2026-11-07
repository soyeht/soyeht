//! Attacks on the connection itself: frames the protocol forbids, requests that skip the
//! handshake, a second handshake, and methods a role does not hold. Each is refused with the
//! exact error code the contract names, and the engine keeps serving the person's own client.
use super::harness_catalogue_and_mcp::{binding_env, mcp_server_for};
use super::*;
use fuzz_support::example;
use soyeht_protocol::limits;

/// A raw connection to the local endpoint that has read the engine's greeting and nothing else.
struct Raw {
    socket: Stream,
}

impl Raw {
    fn connect(rig: &Rig) -> Self {
        let socket = connect(&rig.path.join("engine.sock")).unwrap();
        socket
            .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        socket
            .set_write_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        let mut raw = Self { socket };
        let greeting = raw.read().expect("the engine's greeting");
        assert_eq!(greeting["method"], "auth.challenge");
        raw
    }

    /// The next JSON message, or `None` when the engine closed the connection.
    fn read(&mut self) -> Option<Value> {
        match Frame::read_from(&mut self.socket) {
            Ok(Some(Frame::Json(bytes))) => serde_json::from_slice(&bytes).ok(),
            _ => None,
        }
    }

    /// Whether the engine has closed this connection (nothing but notifications may precede it).
    fn closed(&mut self) -> bool {
        while let Some(message) = self.read() {
            assert!(
                message.get("id").is_none(),
                "an answer to a refused frame: {message}"
            );
        }
        true
    }

    fn write_raw(&mut self, kind: u8, length: u32, body: &[u8]) {
        let mut bytes = vec![kind];
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(body);
        // The engine may close before it has read it all.
        let _ = self.socket.write_all(&bytes);
    }

    fn send(&mut self, value: &Value) {
        let body = serde_json::to_vec(value).unwrap();
        self.write_raw(limits::FRAME_KIND_JSON, body.len() as u32, &body);
    }

    /// The answer to request `id`, skipping notifications.
    fn answer(&mut self, id: u64) -> Value {
        loop {
            let message = self.read().expect("an answer, not a closed connection");
            if message["id"] == id {
                return message;
            }
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        self.answer(id)
    }

    fn hello(&mut self, id: u64, params: Value) -> Value {
        self.request(id, "hello", params)
    }
}

fn kind(answer: &Value) -> &str {
    answer["error"]["data"]["kind"]
        .as_str()
        .unwrap_or("<no error>")
}

fn assert_engine_still_serves(rig: &Rig) {
    let mut client = rig.client();
    assert!(client.call("engine.status", json!({})).is_object());
}

fn schema() -> Value {
    serde_json::from_str(soyeht_protocol::SCHEMA).unwrap()
}

/// Every method of the contract with parameters that satisfy its schema.
fn methods_with_valid_parameters() -> Vec<(String, Value)> {
    let schema = schema();
    schema["x-methods"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, definition)| (name.clone(), example(&schema, &definition["params"], 0)))
        .collect()
}

#[test]
fn a_frame_the_protocol_forbids_closes_the_connection_before_its_payload_is_read() {
    let rig = Rig::new();
    let over = |kind: u8, header: usize, payload: usize| (kind, (header + payload + 1) as u32);
    let cases = [
        over(limits::FRAME_KIND_JSON, 0, limits::JSON_BYTES),
        over(
            limits::FRAME_KIND_TERMINAL,
            limits::TERMINAL_HEADER_BYTES,
            limits::TERMINAL_PAYLOAD_BYTES,
        ),
        over(
            limits::FRAME_KIND_ATTACHMENT,
            limits::ATTACHMENT_HEADER_BYTES,
            limits::ATTACHMENT_PAYLOAD_BYTES,
        ),
        (limits::FRAME_KIND_JSON, u32::MAX),
        (limits::FRAME_KIND_JSON, 0),
        (
            limits::FRAME_KIND_TERMINAL,
            limits::TERMINAL_HEADER_BYTES as u32 - 1,
        ),
        (0, 1),
        (4, 1),
        (255, 1),
    ];
    for (kind, length) in cases {
        let mut raw = Raw::connect(&rig);
        // Only the five header bytes are sent: the engine must decide from them alone.
        raw.write_raw(kind, length, &[]);
        assert!(raw.closed(), "kind {kind} length {length}");
    }
    assert_engine_still_serves(&rig);
}

#[test]
fn a_frame_cut_short_or_followed_by_garbage_ends_only_its_own_connection() {
    let rig = Rig::new();
    let mut truncated = Raw::connect(&rig);
    truncated.write_raw(limits::FRAME_KIND_JSON, 100, b"{\"jsonrpc\"");
    truncated
        .socket
        .shutdown(std::net::Shutdown::Write)
        .unwrap();
    assert!(truncated.closed());
    let mut garbage = Raw::connect(&rig);
    garbage.write_raw(limits::FRAME_KIND_JSON, 7, b"not json");
    assert!(garbage.closed() || garbage.read().is_none());
    assert_engine_still_serves(&rig);
}

#[test]
fn requests_that_are_not_well_formed_json_rpc_are_refused_as_invalid_params() {
    let rig = Rig::new();
    let mut raw = Raw::connect(&rig);
    raw.hello(
        100,
        json!({"client":"attack","clientVersion":"1","protocolVersion":1}),
    );
    for (id, request) in [
        (
            1,
            json!({"jsonrpc":"1.0","id":1,"method":"engine.status","params":{}}),
        ),
        (2, json!({"id":2,"method":"engine.status","params":{}})),
        (4, json!({"jsonrpc":"2.0","id":4,"method":7,"params":{}})),
        (
            5,
            json!({"jsonrpc":"2.0","id":5,"method":"no.such.method","params":{}}),
        ),
        (
            6,
            json!({"jsonrpc":"2.0","id":6,"method":"engine.status","params":[1,2]}),
        ),
        (
            8,
            json!({"jsonrpc":"2.0","id":8,"method":"engine.status","params":{"x":1},"extra":[]}),
        ),
    ] {
        raw.send(&request);
        let answer = raw.answer(id);
        if id == 8 {
            // Unknown members of the envelope and of the parameters are tolerated.
            assert!(answer.get("result").is_some(), "{answer}");
        } else {
            assert_eq!(kind(&answer), "invalid_params", "{request}: {answer}");
        }
    }
    // The connection is still the person's own after every one of them.
    assert!(
        raw.request(9, "engine.status", json!({}))
            .get("result")
            .is_some()
    );
    assert_engine_still_serves(&rig);
}

#[test]
fn a_message_that_is_not_json_or_has_no_method_ends_its_connection() {
    let rig = Rig::new();
    for body in [
        &b"not json"[..],
        b"[]",
        b"7",
        b"\xff\xfe",
        br#"{"jsonrpc":"2.0","id":3,"params":{}}"#,
    ] {
        let mut raw = Raw::connect(&rig);
        raw.hello(
            1,
            json!({"client":"attack","clientVersion":"1","protocolVersion":1}),
        );
        raw.write_raw(limits::FRAME_KIND_JSON, body.len() as u32, body);
        assert!(raw.closed(), "{}", String::from_utf8_lossy(body));
    }
    assert_engine_still_serves(&rig);
}

#[test]
fn a_hello_with_malformed_parameters_is_refused_and_closes_the_connection() {
    let rig = Rig::new();
    for params in [
        json!({"client":1}),
        json!({}),
        json!({"client":"a","clientVersion":"1","protocolVersion":"1"}),
        json!({"client":"a","clientVersion":"1","protocolVersion":1,"capabilities":["web_host","web_host"]}),
        json!({"client":"a","clientVersion":"1","protocolVersion":1,"capabilities":["root"]}),
    ] {
        let mut raw = Raw::connect(&rig);
        let answer = raw.hello(1, params.clone());
        assert_eq!(kind(&answer), "invalid_params", "{params}: {answer}");
        assert!(raw.closed(), "{params}");
    }
}

#[test]
fn every_method_is_refused_before_the_hello_and_nothing_is_changed() {
    let rig = Rig::new();
    let methods = methods_with_valid_parameters();
    assert!(methods.len() > 100);
    let mut raw = Raw::connect(&rig);
    let mut id = 100;
    for (method, params) in methods {
        // The handshake and the pairing exchange are how a connection becomes known.
        if method == "hello" || method.starts_with("pair.") {
            continue;
        }
        id += 1;
        let answer = raw.request(id, &method, params);
        assert_eq!(
            kind(&answer),
            "unauthorized",
            "{method} before hello: {answer}"
        );
        assert_eq!(answer["error"]["message"], "Handshake required", "{method}");
    }
    // Nothing the refused requests named was created.
    let mut client = rig.client();
    assert!(client.section("workspaces").is_empty());
    assert!(client.section("panes").is_empty());
}

#[test]
fn a_second_hello_is_refused_and_ends_the_connection() {
    let rig = Rig::new();
    let base = json!({"client":"attack","clientVersion":"1","protocolVersion":1});
    let mut again = vec![base.clone()];
    for (name, value) in [
        ("binding", json!("x")),
        ("capabilities", json!(["web_host"])),
        ("deviceToken", json!("t")),
    ] {
        let mut hello = base.clone();
        hello[name] = value;
        again.push(hello);
    }
    for second in again {
        let mut raw = Raw::connect(&rig);
        let first = raw.hello(1, base.clone());
        assert_eq!(first["result"]["role"], "human_local");
        let refused = raw.hello(2, second.clone());
        assert_eq!(kind(&refused), "unauthorized", "{second}: {refused}");
        assert_eq!(refused["error"]["message"], "Handshake already completed");
        assert!(
            raw.closed(),
            "a refused handshake keeps the connection: {second}"
        );
    }
    // The refused claims to host the web did not take the single place.
    let _host = Client::new_with_capabilities(rig.path.join("engine.sock"), json!(["web_host"]));
}

#[test]
fn a_hello_for_another_protocol_version_is_refused_and_gives_no_role() {
    let rig = Rig::new();
    for version in [0, 2, 99, u32::MAX] {
        let mut raw = Raw::connect(&rig);
        let answer = raw.hello(
            1,
            json!({"client":"attack","clientVersion":"1","protocolVersion":version}),
        );
        assert_eq!(kind(&answer), "protocol_version_unsupported", "{answer}");
        assert!(raw.closed());
    }
}

#[test]
fn an_unknown_binding_gives_no_role_and_ends_the_connection() {
    let rig = Rig::new();
    for binding in ["not-issued", "", &"a".repeat(5000)] {
        let mut raw = Raw::connect(&rig);
        let answer = raw.hello(
            1,
            json!({"client":"a","clientVersion":"1","protocolVersion":1,"binding":binding}),
        );
        assert!(
            matches!(kind(&answer), "unauthorized" | "invalid_params"),
            "{answer}"
        );
        assert!(raw.closed());
    }
}

#[test]
fn an_agent_bound_connection_holds_only_the_methods_of_its_role() {
    let rig = Rig::new();
    let (mut human, pane) = rig.agent();
    let binding = binding_env(&mcp_server_for(&rig, &mut human, &pane));
    let (mut socket, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "agent");
    // What an agent may call: the task and web families, the agent roster and messages, and a
    // writing structure, a variant and the loop. Every other method is the person's alone.
    let allowed = |method: &str| {
        method.starts_with("task.")
            || method.starts_with("web.")
            || matches!(
                method,
                "agent.list"
                    | "agent.message"
                    | "writing.structure"
                    | "writing.variant.add"
                    | "loop.set"
            )
    };
    let panes_before = human.section("panes");
    let workspaces_before = human.section("workspaces");
    let mut id = 100;
    for (method, params) in methods_with_valid_parameters() {
        if method == "hello" || method.starts_with("pair.") || allowed(&method) {
            continue;
        }
        id += 1;
        let answer = binding_request(&mut socket, id, &method, params);
        assert_eq!(
            kind(&answer),
            "forbidden_role",
            "agent called {method}: {answer}"
        );
    }
    // None of the refused calls reached the engine's state.
    assert_eq!(human.section("panes"), panes_before);
    assert_eq!(human.section("workspaces"), workspaces_before);
}
