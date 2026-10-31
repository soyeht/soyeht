//! Deterministic ACP v1 peer. It only operates in an explicitly selected test directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod error;
mod mcp;
mod skills;
mod state;
mod subagents;
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

/// The protocol sign-in `--session-auth` offers.
const SESSION_AUTH_METHOD: &str = "existing-login";

pub struct Runtime {
    settings: Settings,
    output: Mutex<io::Stdout>,
    sessions: Mutex<BTreeMap<String, Session>>,
    replies: Mutex<BTreeMap<String, mpsc::Sender<Value>>>,
    active: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
    mcp_clients: Mutex<BTreeMap<String, mcp::Client>>,
    native_subagents: AtomicBool,
    /// Whether this process has been authenticated (`--session-auth`).
    authenticated: AtomicBool,
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

    fn initialize(&self, id: Value, params: &Value) {
        self.native_subagents.store(
            subagents::client_supports(&params["clientCapabilities"]),
            Ordering::SeqCst,
        );
        let directory = self.settings.directory.to_string_lossy();
        let methods = if self.settings.session_auth {
            json!([{"id":SESSION_AUTH_METHOD,"name":"Existing login"}])
        } else {
            json!([{"id":"login","name":"Test sign in","type":"terminal","args":["--state-dir",directory,"--login"]}])
        };
        self.answer(id,json!({"protocolVersion":1,"agentInfo":{"name":"Soyeht Test Agent","version":self.settings.version},
            "agentCapabilities":{"loadSession":self.settings.load!="unsupported",
                "auth":if self.settings.logout { json!({"logout":{}}) } else { json!({}) },
                "sessionCapabilities":{"additionalDirectories":{}},
                "promptCapabilities":self.settings.prompt_capabilities.iter()
                    .map(|flag|(flag.clone(),json!(true))).collect::<serde_json::Map<_,_>>()},
            "testPadding":"x".repeat(self.settings.initialize_padding_bytes),
            "authMethods":methods}));
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
        let mut session = Session::new(cwd.into(), Vec::new(), None);
        if !self.attach(&id, &mut session, params) {
            return;
        }
        self.answer(id, session.response(&self.settings));
        skills::advertise(self, &session);
    }

    /// Takes the request's MCP servers, metadata and additional directories,
    /// starts the session's MCP client and registers the session. On failure
    /// it answers the request with the error and returns false.
    fn attach(&self, id: &Value, session: &mut Session, params: &Value) -> bool {
        session.mcp_servers = params["mcpServers"].as_array().cloned().unwrap_or_default();
        session.session_meta = params.get("_meta").cloned();
        session.additional_directories = skills::directories(params);
        let Ok(mcp) = mcp::Client::from_servers(&session.mcp_servers) else {
            self.error(id.clone(), -32603, "Could not start session MCP server");
            return false;
        };
        if session.save(&self.settings).is_err() {
            self.error(id.clone(), -32603, "Could not persist fake session");
            return false;
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
        true
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
                // Like real adapters, a reloaded session starts in the default mode.
                session.values.insert("mode".into(), json!("ask"));
                if !self.attach(&id, &mut session, params) {
                    return;
                }
                for (index, (prompt, text)) in session.history.iter().enumerate() {
                    self.replay_user(&session.id, index, prompt);
                    for block in session.attached.get(&index).into_iter().flatten() {
                        self.update(
                            &session.id,
                            json!({"sessionUpdate":"user_message_chunk","content":block}),
                        );
                    }
                    if let Some(script) = subagents::script(prompt, &session.id, index) {
                        subagents::emit(self, &session.id, script);
                    } else if prompt == "separate-message-ids" {
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
                let mut response = session.response(&self.settings);
                response.as_object_mut().unwrap().remove("sessionId");
                self.answer(id, response);
                skills::advertise(self, &session);
            }
            Err(_) => self.error(id, -32002, "Session not found"),
        }
    }

    fn replay_user(&self, session: &str, index: usize, prompt: &str) {
        let Some(size) = self.settings.replay_user_chunk_bytes else {
            self.update(session,json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":prompt}}));
            return;
        };
        let message_id = format!("user-{index}");
        let mut rest = prompt.trim();
        while !rest.is_empty() {
            let mut end = size.min(rest.len());
            while !rest.is_char_boundary(end) {
                end += 1;
            }
            self.update(session,json!({"sessionUpdate":"user_message_chunk","messageId":message_id,"content":{"type":"text","text":&rest[..end]}}));
            rest = &rest[end..];
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
                Some("model-a" | "model-b" | "delayed-model" | "mode-shifting-model")
            ),
            "effort" => matches!(value.as_str(), Some("low" | "medium" | "high")),
            "mode" => {
                self.settings.announces_mode_option()
                    && value
                        .as_str()
                        .is_some_and(|mode| self.settings.accepts_mode(mode))
            }
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
        if value == "mode-shifting-model" {
            // Like an adapter whose model change resets its mode: the agent
            // announces the new mode before it confirms the model.
            session.values.insert("mode".into(), json!("accept_edits"));
            let update = json!({"sessionUpdate":"config_option_update",
                "configOptions":session.options(&self.settings)});
            self.update(session_id, update);
        }
        let response = json!({"configOptions":session.options(&self.settings)});
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

    fn set_mode(&self, id: Value, params: &Value) {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = params["sessionId"]
            .as_str()
            .and_then(|session| sessions.get_mut(session))
        else {
            self.error(id, -32002, "Session not found");
            return;
        };
        let mode = params["modeId"].as_str().unwrap_or("");
        if self.settings.modes != "legacy" || !self.settings.accepts_mode(mode) {
            self.error(id, -32602, "Mode refused");
            return;
        }
        session.values.insert("mode".into(), json!(mode));
        if session.save(&self.settings).is_err() {
            self.error(id, -32603, "Could not save mode");
            return;
        }
        self.answer(id, json!({}));
    }

    /// Changes the mode as the agent itself would, then announces it.
    pub(crate) fn switch_mode(&self, session_id: &str, mode: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(session_id) else {
            return;
        };
        session.values.insert("mode".into(), json!(mode));
        session.save(&self.settings).expect("fake mode switch");
        let update = if self.settings.modes == "legacy" {
            json!({"sessionUpdate":"current_mode_update","currentModeId":mode})
        } else {
            json!({"sessionUpdate":"config_option_update","configOptions":session.options(&self.settings)})
        };
        drop(sessions);
        self.update(session_id, update);
    }

    pub(crate) fn session_mode(&self, session_id: &str) -> String {
        self.sessions
            .lock()
            .unwrap()
            .get(session_id)
            .map(|session| session.mode().to_owned())
            .unwrap_or_default()
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
        let attached: Vec<_> = prompt
            .iter()
            .filter(|block| block["type"] != "text")
            .cloned()
            .collect();
        if !attached.is_empty()
            && let Some(saved) = self.sessions.lock().unwrap().get_mut(&session)
        {
            let turn = saved.history.len();
            saved.attached.insert(turn, attached);
        }
        if text == "report-blocks" {
            // Every block after the text, as the agent received it.
            let blocks: Vec<_> = prompt.iter().skip(1).cloned().collect();
            text = format!("block-report:{}", Value::Array(blocks));
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
            self.initialize(id, &params);
            return;
        }
        if method == "logout" && self.settings.logout {
            let _ = std::fs::remove_file(self.settings.directory.join("login.ok"));
            self.answer(id, json!({}));
            return;
        }
        if self.settings.session_auth && method == "authenticate" {
            if params["methodId"] == SESSION_AUTH_METHOD && self.settings.logged_in() {
                self.authenticated.store(true, Ordering::SeqCst);
                self.answer(id, json!({}));
            } else {
                self.error(id, -32000, "Authentication required");
            }
            return;
        }
        let opens_session = matches!(method.as_str(), "session/new" | "session/load");
        let unauthenticated = self.settings.session_auth
            && opens_session
            && !self.authenticated.load(Ordering::SeqCst);
        if unauthenticated || !self.settings.logged_in() {
            self.error(id, -32000, "Authentication required");
            return;
        }
        match method.as_str() {
            "session/new" => self.new_session(id, &params),
            "session/load" => self.load_session(id, &params),
            "session/set_config_option" => self.set_config_option(id, &params),
            "session/set_mode" => self.set_mode(id, &params),
            "session/prompt" => self.prompt(id, &params),
            _ => self.error(id, -32601, "Method not found"),
        }
    }
}
/// A fixture nobody released stops at its deadline instead of outliving its test.
fn wait_or_exit(deadline: std::time::Instant) {
    if std::time::Instant::now() >= deadline {
        std::process::exit(1);
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
}

/// Stores the account the sign-in chose (`login.account`, or a default).
fn save_sign_in(settings: &Settings) -> Result<()> {
    let account = std::fs::read_to_string(settings.directory.join("login.account"))
        .unwrap_or_else(|_| "signed in".into());
    std::fs::write(settings.directory.join("login.ok"), account)?;
    Ok(())
}

/// Behaves like a browser login: it owns a localhost listener until the
/// redirect arrives and reports success only after that request.
fn listen_for_callback(settings: &Settings) -> Result<()> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    std::fs::write(
        settings.directory.join("login.port"),
        listener.local_addr()?.port().to_string(),
    )?;
    println!("Test sign-in listening.");
    let deadline = std::time::Instant::now() + FIXTURE_RELEASE_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request)?;
                stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                )?;
                save_sign_in(settings)?;
                println!("Test sign-in complete.");
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_or_exit(deadline);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn main() -> Result<()> {
    let settings = Settings::parse()?;
    if settings.login {
        std::fs::create_dir_all(&settings.directory)?;
        if settings.directory.join("login.listen").exists() {
            return listen_for_callback(&settings);
        }
        let failed = settings.directory.join("login.fail").exists();
        // A real login CLI can exit 0 after its browser flow is cancelled.
        let cancelled = settings.directory.join("login.cancel").exists();
        if failed || cancelled || settings.directory.join("login.wait").exists() {
            println!("Test sign-in waiting.");
            std::fs::write(settings.directory.join("login.started"), b"started")?;
            let deadline = std::time::Instant::now() + FIXTURE_RELEASE_TIMEOUT;
            while !settings.directory.join("login.release").exists() {
                wait_or_exit(deadline);
            }
        }
        if failed {
            println!("Test sign-in failed.");
            std::process::exit(1);
        }
        if cancelled {
            println!("Test sign-in cancelled.");
            return Ok(());
        }
        save_sign_in(&settings)?;
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
        native_subagents: AtomicBool::new(false),
        authenticated: AtomicBool::new(false),
    });
    for line in io::stdin().lock().split(b'\n') {
        match Message::parse(&line?)? {
            Message::Request { id, method, params } => runtime.request(id, method, params),
            Message::Notification { method, params } if method == "session/cancel" => {
                // A cancel that reaches an agent before its turn has begun is dropped, as the
                // real adapters do: the marker makes the first one that.
                if std::fs::remove_file(runtime.settings.directory.join("drop-first-cancel")).is_ok()
                {
                    continue;
                }
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
