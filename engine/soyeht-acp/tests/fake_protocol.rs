#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_acp::{
    Message, RpcError,
    client::{self, EnvVariable, StdioMcpServer},
    config::Options,
    session,
};
use soyeht_protocol::ProbeResult;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use uuid::Uuid;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("soyeht-next-acp-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Peer {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<Message>,
    reader: Option<thread::JoinHandle<()>>,
    next: u64,
    notifications: Vec<Message>,
}
impl Peer {
    fn start(directory: &Directory, args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_soyeht-fake-acp"))
            .arg("--state-dir")
            .arg(&directory.0)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let out = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(out).split(b'\n') {
                let Ok(line) = line else {
                    break;
                };
                let Ok(message) = Message::parse(&line) else {
                    break;
                };
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            reader: Some(reader),
            next: 0,
            notifications: Vec::new(),
        }
    }
    fn send(&mut self, message: Message) {
        self.input.write_all(&message.encode().unwrap()).unwrap();
        self.input.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = json!(self.next);
        self.send(Message::Request {
            id: id.clone(),
            method: method.into(),
            params,
        });
        id
    }
    fn receive(&mut self) -> Message {
        self.output
            .recv_timeout(Duration::from_secs(5))
            .expect("fake agent response deadline")
    }
    fn response(&mut self, expected: &Value) -> Result<Value, RpcError> {
        loop {
            match self.receive() {
                Message::Response { id, result } if id == *expected => return result,
                message @ Message::Notification { .. } => self.notifications.push(message),
                other => panic!("unexpected native message: {other:?}"),
            }
        }
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.request(method, params);
        self.response(&id)
    }
    fn initialize(&mut self) -> session::Initialization {
        let mut client = soyeht_acp::Client::default();
        let message = client.initialize(0).unwrap();
        assert_eq!(
            message.value()["params"]["clientCapabilities"]["session"]["configOptions"]["boolean"],
            json!({})
        );
        self.send(message);
        let response = self.receive();
        let (_, value) = client.response(response).unwrap();
        session::Initialization::parse(&value.unwrap()).unwrap()
    }
    fn session(&mut self, directory: &Directory) -> (String, Options) {
        let response = self
            .call(
                "session/new",
                client::new_session(directory.0.to_str().unwrap()),
            )
            .unwrap();
        (
            session::session_id(&response).unwrap(),
            Options::from_response(&response).unwrap(),
        )
    }
    fn session_with_mcp(&mut self, directory: &Directory, server: &StdioMcpServer) -> String {
        let response = self
            .call(
                "session/new",
                client::new_session_with_mcp(
                    directory.0.to_str().unwrap(),
                    std::slice::from_ref(server),
                ),
            )
            .unwrap();
        session::session_id(&response).unwrap()
    }
    fn native_request(&mut self, expected: &str) -> (Value, Value) {
        loop {
            match self.receive() {
                Message::Request { id, method, params } => {
                    assert_eq!(method, expected);
                    return (id, params);
                }
                message @ Message::Notification { .. } => self.notifications.push(message),
                other => panic!("expected native request, got {other:?}"),
            }
        }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn chunked_prompt_config_confirmation_and_refused_config_keep_the_session() {
    let directory = Directory::new();
    let mut peer = Peer::start(&directory, &[]);
    peer.initialize();
    let (session, options) = peer.session(&directory);
    let prompt = peer
        .call("session/prompt", client::prompt(&session, "hello"))
        .unwrap();
    assert_eq!(session::stop_reason(&prompt).unwrap(), "end_turn");
    let chunks: Vec<_> = peer
        .notifications
        .iter()
        .filter_map(|message| match message {
            Message::Notification { params, .. }
                if params["update"]["sessionUpdate"] == "agent_message_chunk" =>
            {
                Some(params["update"]["content"]["text"].as_str().unwrap())
            }
            _ => None,
        })
        .collect();
    assert_eq!(chunks, ["Echo: ", "hello"]);
    let selected = serde_json::from_value(json!("model-b")).unwrap();
    let (method, params) = options.request(&session, "model", &selected).unwrap();
    let confirmation = peer.call(method, params).unwrap();
    let options = options
        .confirmed("model", &selected, &confirmation)
        .unwrap();
    let selected = serde_json::from_value(json!("high")).unwrap();
    let (method, params) = options.request(&session, "effort", &selected).unwrap();
    assert!(
        options
            .confirmed("effort", &selected, &peer.call(method, params).unwrap())
            .is_ok()
    );
    let error = peer
        .call(
            "session/set_config_option",
            json!({"sessionId":session,"configId":"model","value":"rejected-model"}),
        )
        .unwrap_err();
    assert_eq!(
        session::probe_result(Err(&error)),
        ProbeResult::Inconclusive
    );
    assert!(
        peer.call("session/prompt", client::prompt(&session, "still here"))
            .is_ok()
    );
}

#[test]
fn null_permission_id_and_elicitation_remain_separate_round_trips() {
    let directory = Directory::new();
    let mut peer = Peer::start(&directory, &[]);
    peer.initialize();
    let (session, _) = peer.session(&directory);
    let prompt = peer.request(
        "session/prompt",
        client::prompt(&session, "permission-null"),
    );
    let (id, params) = peer.native_request("session/request_permission");
    assert!(id.is_null());
    assert_eq!(params["sessionId"], session);
    peer.send(client::permission_answer(id, Some("once")));
    assert_eq!(
        session::stop_reason(&peer.response(&prompt).unwrap()).unwrap(),
        "end_turn"
    );
    peer.call(
        "session/set_config_option",
        json!({"sessionId":session,"configId":"mode","value":"allow_all"}),
    )
    .unwrap();
    let prompt = peer.request("session/prompt", client::prompt(&session, "elicitation"));
    let (id, params) = peer.native_request("elicitation/create");
    assert_eq!(params["mode"], "form");
    assert!(params["requestedSchema"]["properties"]["answer"].is_object());
    peer.send(client::elicitation_answer(
        id,
        Some(json!({"answer":"ready"})),
    ));
    assert_eq!(
        session::stop_reason(&peer.response(&prompt).unwrap()).unwrap(),
        "end_turn"
    );
}

#[test]
fn cancellation_finishes_the_response_without_killing_the_process() {
    let directory = Directory::new();
    let mut peer = Peer::start(&directory, &[]);
    peer.initialize();
    let (session, _) = peer.session(&directory);
    let prompt = peer.request("session/prompt", client::prompt(&session, "slow"));
    loop {
        if let Message::Notification { params, .. } = peer.receive()
            && params["update"]["sessionUpdate"] == "agent_message_chunk"
        {
            break;
        }
    }
    peer.send(client::cancel(&session));
    assert_eq!(
        session::stop_reason(&peer.response(&prompt).unwrap()).unwrap(),
        "cancelled"
    );
    assert!(peer.child.try_wait().unwrap().is_none());
    assert!(
        peer.call("session/prompt", client::prompt(&session, "next turn"))
            .is_ok()
    );
}

#[test]
fn loading_after_process_death_replays_the_saved_session() {
    let directory = Directory::new();
    let mut peer = Peer::start(&directory, &[]);
    peer.initialize();
    let (session, _) = peer.session(&directory);
    peer.call("session/prompt", client::prompt(&session, "before restart"))
        .unwrap();
    let old_pid = peer.child.id();
    drop(peer);
    let mut peer = Peer::start(&directory, &[]);
    assert_ne!(old_pid, peer.child.id());
    peer.initialize();
    let loaded = peer
        .call(
            "session/load",
            client::load_session(directory.0.to_str().unwrap(), &session),
        )
        .unwrap();
    assert_eq!(Options::from_response(&loaded).unwrap().values.len(), 4);
    assert!(
        peer.notifications
            .iter()
            .any(|m| m.value()["params"]["update"]["content"]["text"] == "Echo: before restart")
    );
    assert!(
        peer.call("session/prompt", client::prompt(&session, "after restart"))
            .is_ok()
    );
}

#[test]
fn advertised_resume_is_classified_by_an_actual_valid_probe() {
    let directory = Directory::new();
    let mut peer = Peer::start(&directory, &["--load", "advertised-only"]);
    assert!(peer.initialize().advertised_load);
    let (session, _) = peer.session(&directory);
    let result = peer.call(
        "session/load",
        client::load_session(directory.0.to_str().unwrap(), &session),
    );
    assert_eq!(
        session::probe_result(result.as_ref()),
        ProbeResult::Unsupported
    );
}

#[test]
fn login_runs_as_a_separate_terminal_invocation() {
    let directory = Directory::new();
    let mut peer = Peer::start(&directory, &["--require-login"]);
    let info = peer.initialize();
    assert_eq!(info.auth_methods[0].kind, session::AuthKind::Terminal);
    let error = peer
        .call(
            "session/new",
            client::new_session(directory.0.to_str().unwrap()),
        )
        .unwrap_err();
    assert_eq!(
        session::probe_result(Err(&error)),
        ProbeResult::Unauthenticated
    );
    let login = Command::new(env!("CARGO_BIN_EXE_soyeht-fake-acp"))
        .arg("--state-dir")
        .arg(&directory.0)
        .args(&info.auth_methods[0].args)
        .output()
        .unwrap();
    assert!(login.status.success());
    assert!(
        peer.call(
            "session/new",
            client::new_session(directory.0.to_str().unwrap())
        )
        .is_ok()
    );
}

#[test]
fn injected_stdio_mcp_drives_the_deterministic_task_workflow() {
    let directory = Directory::new();
    let log = directory.0.join("task-tools.ndjson");
    let server = StdioMcpServer {
        name: "Soyeht".into(),
        command: env!("CARGO_BIN_EXE_soyeht-fake-mcp").into(),
        args: vec!["--log".into(), log.to_string_lossy().into_owned()],
        env: vec![EnvVariable {
            name: "SOYEHT_TEST_MCP".into(),
            value: "1".into(),
        }],
    };
    let mut peer = Peer::start(&directory, &[]);
    peer.initialize();
    for instruction in ["task-claim", "task-execute", "task-block", "task-drift"] {
        let session = peer.session_with_mcp(&directory, &server);
        let result = peer
            .call("session/prompt", client::prompt(&session, instruction))
            .unwrap();
        assert_eq!(session::stop_reason(&result).unwrap(), "end_turn");
    }

    let calls: Vec<Value> = fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        calls
            .iter()
            .map(|call| call["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "task_next",
            "task_subitem_add",
            "task_subitem_add",
            "task_comment",
            "task_subitem_done",
            "task_subitem_done",
            "task_request_review",
            "task_complete",
            "task_block",
            "task_complete",
        ]
    );
    assert_eq!(calls[0]["arguments"], json!({}));
    assert_eq!(
        calls[3]["arguments"]["text"],
        "Planning milestone completed"
    );
    assert_eq!(calls[7]["arguments"]["contractChanges"], json!([]));
    assert_eq!(calls[7]["arguments"]["outOfScopeEdits"], json!([]));
    assert_eq!(calls[8]["arguments"]["kind"], "missing_information");
    assert_eq!(
        calls[9]["arguments"]["contractChanges"][0]["label"],
        "silent_fallback"
    );
    assert_eq!(
        calls[9]["arguments"]["outOfScopeEdits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
