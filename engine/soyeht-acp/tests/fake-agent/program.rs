//! Deterministic ACP v1 peer. It only operates in an explicitly selected test directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod error;
mod mcp;
mod state;
mod turns;

use error::Result;
use serde_json::{Value, json};
use soyeht_acp::{Message, RpcError};
use state::{Session, Settings};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

// The fake waits for a test-controlled release signal. A larger failure
// budget does not delay a passing run.
pub(crate) const FIXTURE_RELEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct Runtime {
    settings: Settings,
    output: Mutex<io::Stdout>,
    sessions: Mutex<BTreeMap<String, Session>>,
    replies: Mutex<BTreeMap<String, mpsc::Sender<Value>>>,
    active: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
    mcp_clients: Mutex<BTreeMap<String, mcp::Client>>,
}
impl Runtime {
    fn send(&self, message: Message) {
        let bytes = message.encode().expect("bounded fake message");
        let mut out = self.output.lock().expect("output lock");
        if out.write_all(&bytes).and_then(|_| out.flush()).is_err() {
            std::process::exit(0);
        }
    }
    fn answer(&self, id: Value, result: Value) {
        self.send(Message::Response {
            id,
            result: Ok(result),
        });
    }
    fn error(&self, id: Value, code: i32, message: &str) {
        self.error_with_data(id, code, message, None);
    }
    fn error_with_data(&self, id: Value, code: i32, message: &str, data: Option<Value>) {
        self.send(Message::Response {
            id,
            result: Err(RpcError {
                code,
                message: message.into(),
                data,
            }),
        });
    }
    fn update(&self, session: &str, update: Value) {
        self.send(Message::Notification {
            method: "session/update".into(),
            params: json!({"sessionId":session,"update":update}),
        });
    }

    fn initialize(&self, id: Value) {
        let directory = self.settings.directory.to_string_lossy();
        self.answer(id,json!({"protocolVersion":1,"agentInfo":{"name":"Soyeht Test Agent","version":self.settings.version},
            "agentCapabilities":{"loadSession":self.settings.load!="unsupported"},
            "testPadding":"x".repeat(self.settings.initialize_padding_bytes),
            "authMethods":[{"id":"login","name":"Test sign in","type":"terminal","args":["--state-dir",directory,"--login"]}]}));
    }

    fn new_session(&self, id: Value, params: &Value) {
        let Some(cwd) = params["cwd"]
            .as_str()
            .filter(|cwd| std::path::Path::new(cwd).is_absolute())
        else {
            self.error(id, -32602, "Absolute cwd required");
            return;
        };
        if !params["mcpServers"].is_array() {
            self.error(id, -32602, "mcpServers required");
            return;
        }
        let servers = params["mcpServers"].as_array().cloned().unwrap_or_default();
        let session = Session::new(cwd.into(), servers, params.get("_meta").cloned());
        let mcp = match mcp::Client::from_servers(&session.mcp_servers) {
            Ok(mcp) => mcp,
            Err(_) => {
                self.error(id, -32603, "Could not start session MCP server");
                return;
            }
        };
        if session.save(&self.settings).is_err() {
            self.error(id, -32603, "Could not persist fake session");
            return;
        }
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        if let Some(mcp) = mcp {
            self.mcp_clients
                .lock()
                .unwrap()
                .insert(session.id.clone(), mcp);
        }
        self.answer(id, session.response());
    }

    fn load_session(&self, id: Value, params: &Value) {
        match self.settings.load.as_str() {
            "unsupported" | "advertised-only" => {
                self.error(id, -32601, "Method not found");
                return;
            }
            "missing" => {
                self.error(id, -32002, "Session not found");
                return;
            }
            "timeout" => return,
            _ => (),
        }
        let Some(session_id) = params["sessionId"].as_str() else {
            self.error(id, -32602, "Session ID required");
            return;
        };
        match Session::load(&self.settings, session_id) {
            Ok(mut session) => {
                session.mcp_servers = params["mcpServers"].as_array().cloned().unwrap_or_default();
                session.session_meta = params.get("_meta").cloned();
                let mcp = match mcp::Client::from_servers(&session.mcp_servers) {
                    Ok(mcp) => mcp,
                    Err(_) => {
                        self.error(id, -32603, "Could not start session MCP server");
                        return;
                    }
                };
                if session.save(&self.settings).is_err() {
                    self.error(id, -32603, "Could not persist fake session");
                    return;
                }
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(session.id.clone(), session.clone());
                if let Some(mcp) = mcp {
                    self.mcp_clients
                        .lock()
                        .unwrap()
                        .insert(session.id.clone(), mcp);
                }
                for (prompt, text) in &session.history {
                    self.update(&session.id,json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":prompt}}));
                    if prompt == "separate-message-ids" {
                        for (message_id, chunk) in [
                            ("first", "Failed to authenticate: OAuth session expired"),
                            ("second", "No response requested."),
                        ] {
                            self.update(&session.id,json!({"sessionUpdate":"agent_message_chunk","messageId":message_id,"content":{"type":"text","text":chunk}}));
                        }
                    } else {
                        self.update(&session.id,json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}));
                    }
                }
                self.answer(id, json!({"configOptions":session.options()}));
            }
            Err(_) => self.error(id, -32002, "Session not found"),
        }
    }

    fn set_config_option(self: &Arc<Self>, id: Value, params: &Value) {
        let Some(session_id) = params["sessionId"].as_str() else {
            self.error(id, -32602, "Session ID required");
            return;
        };
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(session_id) else {
            self.error(id, -32002, "Session not found");
            return;
        };
        let config = params["configId"].as_str().unwrap_or("");
        let value = &params["value"];
        let allowed = match config {
            "model" => matches!(
                value.as_str(),
                Some("model-a" | "model-b" | "delayed-model")
            ),
            "effort" => matches!(value.as_str(), Some("low" | "medium" | "high")),
            "mode" => matches!(value.as_str(), Some("ask" | "accept_edits" | "allow_all")),
            "fast" => value.is_boolean() && params["type"] == "boolean",
            _ => false,
        };
        if !allowed {
            self.error(id, -32602, "Configuration value refused");
            return;
        }
        let mut next = session.clone();
        next.values.insert(config.into(), value.clone());
        if next.save(&self.settings).is_err() {
            self.error(id, -32603, "Could not save configuration");
            return;
        }
        *session = next;
        let response = json!({"configOptions":session.options()});
        if value == "delayed-model" {
            let runtime = self.clone();
            std::fs::write(self.settings.directory.join("config-waiting"), b"ready").unwrap();
            std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + FIXTURE_RELEASE_TIMEOUT;
                while !runtime.settings.directory.join("config-release").exists() {
                    if std::time::Instant::now() >= deadline {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                runtime.answer(id, response);
            });
        } else {
            self.answer(id, response);
        }
    }

    fn prompt(self: &Arc<Self>, id: Value, params: &Value) {
        let session = params["sessionId"].as_str().unwrap_or("").to_owned();
        if !self.sessions.lock().unwrap().contains_key(&session) {
            self.error(id, -32002, "Session not found");
            return;
        }
        let Some(prompt) = params["prompt"].as_array() else {
            self.error(id, -32602, "Prompt blocks required");
            return;
        };
        let mut text = prompt
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if text == "report-images" {
            let images: Vec<_> = prompt
                .iter()
                .filter(|block| block["type"] == "image")
                .map(|block| {
                    json!({"mimeType":block["mimeType"],"data":block["data"],
                        "hasUri":block.get("uri").is_some()})
                })
                .collect();
            text = format!("image-report:{}", Value::Array(images));
        }
        if text == "refuse-disabled-model" {
            self.error(id, -32603, "Internal error: Model is disabled");
            return;
        }
        if text == "refuse-generic-error" {
            self.error(id, -32603, "Internal error: Temporary upstream failure");
            return;
        }
        if text == "auth-expired" {
            self.error_with_data(
                id,
                -32603,
                "Failed to authenticate: OAuth session expired",
                Some(json!({"errorKind":"authentication_failed"})),
            );
            return;
        }
        if text == "auth-expired-with-chunk" {
            self.update(&session, json!({"sessionUpdate":"agent_message_chunk","messageId":"auth-error","content":{"type":"text","text":"Failed to authenticate: OAuth session expired"}}));
            self.error_with_data(
                id,
                -32603,
                "Failed to authenticate: OAuth session expired",
                Some(json!({"errorKind":"authentication_failed"})),
            );
            return;
        }
        if text == "auth-expired-untagged" {
            self.error(id, -32603, "Failed to authenticate: OAuth session expired");
            return;
        }
        let mut active = self.active.lock().unwrap();
        if active.contains_key(&session) {
            self.error(id, -32600, "Session already has an active turn");
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        active.insert(session.clone(), cancelled.clone());
        let runtime = self.clone();
        std::thread::spawn(move || turns::run(runtime, id, session, text, cancelled));
    }

    fn request(self: &Arc<Self>, id: Value, method: String, params: Value) {
        if method == "initialize" {
            self.initialize(id);
            return;
        }
        if !self.settings.logged_in() {
            self.error(id, -32000, "Authentication required");
            return;
        }
        match method.as_str() {
            "session/new" => self.new_session(id, &params),
            "session/load" => self.load_session(id, &params),
            "session/set_config_option" => self.set_config_option(id, &params),
            "session/prompt" => self.prompt(id, &params),
            _ => self.error(id, -32601, "Method not found"),
        }
    }
}
fn main() -> Result<()> {
    let settings = Settings::parse()?;
    if settings.login {
        std::fs::create_dir_all(&settings.directory)?;
        std::fs::write(settings.directory.join("login.ok"), b"signed in\n")?;
        println!("Test sign-in complete.");
        return Ok(());
    }
    let runtime = Arc::new(Runtime {
        settings,
        output: Mutex::new(io::stdout()),
        sessions: Mutex::new(BTreeMap::new()),
        replies: Mutex::new(BTreeMap::new()),
        active: Mutex::new(BTreeMap::new()),
        mcp_clients: Mutex::new(BTreeMap::new()),
    });
    for line in io::stdin().lock().split(b'\n') {
        match Message::parse(&line?)? {
            Message::Request { id, method, params } => runtime.request(id, method, params),
            Message::Notification { method, params } if method == "session/cancel" => {
                if let Some(cancelled) = params["sessionId"]
                    .as_str()
                    .and_then(|id| runtime.active.lock().unwrap().get(id).cloned())
                {
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
            Message::Response { id, result } => {
                if let Some(sender) = runtime.replies.lock().unwrap().remove(&id.to_string()) {
                    let _ = sender.send(match result {
                        Ok(v) => v,
                        Err(error) => json!({"error":error}),
                    });
                }
            }
            _ => (),
        }
    }
    Ok(())
}
