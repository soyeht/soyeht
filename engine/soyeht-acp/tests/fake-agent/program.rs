//! Deterministic ACP v1 peer. It only operates in an explicitly selected test directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod error;
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

pub struct Runtime {
    settings: Settings,
    output: Mutex<io::Stdout>,
    sessions: Mutex<BTreeMap<String, Session>>,
    replies: Mutex<BTreeMap<String, mpsc::Sender<Value>>>,
    active: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
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
        self.send(Message::Response {
            id,
            result: Err(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        });
    }
    fn update(&self, session: &str, update: Value) {
        self.send(Message::Notification {
            method: "session/update".into(),
            params: json!({"sessionId":session,"update":update}),
        });
    }

    fn request(self: &Arc<Self>, id: Value, method: String, params: Value) {
        if method == "initialize" {
            self.answer(id,json!({"protocolVersion":1,"agentInfo":{"name":"Soyeht Test Agent","version":self.settings.version},
                "agentCapabilities":{"loadSession":self.settings.load!="unsupported"},
                "authMethods":[{"id":"login","name":"Test sign in","type":"terminal","args":["--login"]}]}));
            return;
        }
        if !self.settings.logged_in() {
            self.error(id, -32000, "Authentication required");
            return;
        }
        match method.as_str() {
            "session/new" => {
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
                let session = Session::new(cwd.into());
                if session.save(&self.settings).is_err() {
                    self.error(id, -32603, "Could not persist fake session");
                    return;
                }
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(session.id.clone(), session.clone());
                self.answer(id, session.response());
            }
            "session/load" => {
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
                    Ok(session) => {
                        self.sessions
                            .lock()
                            .unwrap()
                            .insert(session.id.clone(), session.clone());
                        for (prompt, text) in &session.history {
                            self.update(&session.id,json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":prompt}}));
                            self.update(&session.id,json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}));
                        }
                        self.answer(id, json!({"configOptions":session.options()}));
                    }
                    Err(_) => self.error(id, -32002, "Session not found"),
                }
            }
            "session/set_config_option" => {
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
                    std::fs::write(self.settings.directory.join("config-waiting"), b"ready")
                        .unwrap();
                    std::thread::spawn(move || {
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(20);
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
            "session/prompt" => {
                let session = params["sessionId"].as_str().unwrap_or("").to_owned();
                if !self.sessions.lock().unwrap().contains_key(&session) {
                    self.error(id, -32002, "Session not found");
                    return;
                }
                let Some(prompt) = params["prompt"].as_array() else {
                    self.error(id, -32602, "Prompt blocks required");
                    return;
                };
                let text = prompt
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
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
