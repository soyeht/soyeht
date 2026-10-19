use crate::{FIXTURE_RELEASE_TIMEOUT, Message, Runtime};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use uuid::Uuid;

fn call(
    runtime: &Runtime,
    session: &str,
    method: &str,
    mut params: Value,
    cancelled: &AtomicBool,
    null_id: bool,
) -> Option<Value> {
    let id = if null_id {
        Value::Null
    } else {
        json!(Uuid::new_v4().to_string())
    };
    params["sessionId"] = json!(session);
    let (sender, receiver) = mpsc::channel();
    runtime
        .replies
        .lock()
        .unwrap()
        .insert(id.to_string(), sender);
    runtime.send(Message::Request {
        id: id.clone(),
        method: method.into(),
        params,
    });
    loop {
        if cancelled.load(Ordering::SeqCst) {
            runtime.replies.lock().unwrap().remove(&id.to_string());
            return None;
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => return Some(result),
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(_) => return None,
        }
    }
}

fn emit(runtime: &Runtime, session: &str, text: &str) {
    runtime.update(
        session,
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}),
    );
}

fn permission_turn(
    runtime: &Arc<Runtime>,
    session: &str,
    text: &str,
    cancelled: &AtomicBool,
) -> String {
    if text == "permission-gated" {
        std::fs::write(
            runtime.settings.directory.join("permission.waiting"),
            b"waiting",
        )
        .expect("fake permission receipt marker");
        let deadline = std::time::Instant::now() + FIXTURE_RELEASE_TIMEOUT;
        while !runtime
            .settings
            .directory
            .join("permission.release")
            .exists()
        {
            if cancelled.load(Ordering::SeqCst) || std::time::Instant::now() >= deadline {
                return String::new();
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    if text == "permission-overflow" {
        thread::sleep(Duration::from_millis(500));
        let flood = Arc::clone(runtime);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            for _ in 0..160 {
                flood.send(Message::Notification {
                    method: "test/output".into(),
                    params: json!({"text":"x".repeat(64 * 1024)}),
                });
            }
            std::fs::write(flood.settings.directory.join("overflow.done"), b"done")
                .expect("fake overflow marker");
        });
    }
    if runtime.session_mode(session) == "allow_all" {
        let output = "Permission result: approved by the agent's own mode".to_owned();
        emit(runtime, session, &output);
        return output;
    }
    let edit = text == "permission-edit";
    let mut tool_call = json!({"toolCallId":"permission-tool","title":"Run test operation",
        "kind":if edit{"edit"}else{"execute"},"status":"pending"});
    // codex-acp 1.11.0 reports an MCP call as a tool_call and then asks to
    // approve it with only the id, kind and status. A subagent does both
    // from its own session.
    let subagent_tool = text.strip_prefix("permission-subagent-mcp:");
    let asking = match subagent_tool {
        Some(_) => {
            let turn = runtime.sessions.lock().unwrap()[session].history.len();
            let child = format!("{session}-asking-{turn}");
            runtime.update(
                session,
                crate::subagents::spawned(&child, "Messenger", "Use the workspace tools"),
            );
            child
        }
        None => session.to_owned(),
    };
    let mcp_tool = text
        .strip_prefix("permission-tracked-mcp:")
        .or(subagent_tool);
    if let Some(tool) = mcp_tool {
        let id = format!("exec-{tool}");
        runtime.update(
            &asking,
            json!({"sessionUpdate":"tool_call","toolCallId":id,"kind":"execute",
                "title":format!("mcp.soyeht-workspace.{tool}"),"status":"in_progress",
                "rawInput":{"server":"soyeht-workspace","tool":tool,
                    "arguments":{"text":"  first line\n  second line"}},
                "_meta":{"is_mcp_tool_call":true}}),
        );
        tool_call = json!({"toolCallId":id,"kind":"execute","status":"pending",
            "_meta":{"is_mcp_tool_approval":true}});
    } else if text == "permission-untracked-mcp" {
        tool_call = json!({"toolCallId":"exec-never-reported","kind":"execute","status":"pending"});
    } else if !edit && text != "permission-missing-command" {
        tool_call["rawInput"] = json!({"command":"printf 'test operation'"});
    }
    let result = call(
        runtime,
        &asking,
        "session/request_permission",
        json!({"toolCall":tool_call,"options":[{"optionId":"once","kind":"allow_once","name":"Allow once"},{"optionId":"always","kind":"allow_always","name":"Always allow"},{"optionId":"deny","kind":"reject_once","name":"Deny"}]}),
        cancelled,
        text == "permission-null",
    );
    if asking != session {
        runtime.update(session, crate::subagents::state(&asking, "completed"));
    }
    result.map_or_else(String::new, |result| {
        let output = format!("Permission result: {}", result["outcome"]);
        emit(runtime, session, &output);
        output
    })
}

fn elicitation_turn(runtime: &Runtime, session: &str, cancelled: &AtomicBool) -> String {
    let result = call(
        runtime,
        session,
        "elicitation/create",
        json!({"mode":"form","message":"Choose a test answer","requestedSchema":{"type":"object","properties":{"answer":{"type":"string","title":"Answer"}},"required":["answer"]}}),
        cancelled,
        false,
    );
    result.map_or_else(String::new, |result| {
        let output = format!("Form result: {result}");
        emit(runtime, session, &output);
        output
    })
}

fn missing_terminal_turn(runtime: &Runtime, session: &str, cancelled: &AtomicBool) -> String {
    let result = call(
        runtime,
        session,
        "terminal/create",
        json!({"command":runtime.settings.directory.join("nonexistent-command")}),
        cancelled,
        false,
    );
    let output = format!("Terminal creation: {}", result.unwrap_or(Value::Null));
    emit(runtime, session, &output);
    output
}

#[cfg(unix)]
const SHELL: &str = "/bin/sh";
#[cfg(unix)]
const SHELL_FLAG: &str = "-c";
#[cfg(unix)]
const PRINT_TEST_TEXT: &str = "printf fake-terminal";
#[cfg(windows)]
const SHELL: &str = "cmd.exe";
#[cfg(windows)]
const SHELL_FLAG: &str = "/c";
#[cfg(windows)]
const PRINT_TEST_TEXT: &str = "<nul set /p =fake-terminal";

fn tools_turn(runtime: &Runtime, session: &str, cancelled: &AtomicBool) -> String {
    let cwd = runtime.sessions.lock().unwrap()[session].cwd.clone();
    let path = std::path::Path::new(&cwd).join("fake-acp-note.txt");
    let _ = call(
        runtime,
        session,
        "fs/write_text_file",
        json!({"path":path,"content":"test data\n"}),
        cancelled,
        false,
    );
    let read = call(
        runtime,
        session,
        "fs/read_text_file",
        json!({"path":path}),
        cancelled,
        false,
    );
    let terminal = call(
        runtime,
        session,
        "terminal/create",
        json!({"command":SHELL,"args":[SHELL_FLAG,PRINT_TEST_TEXT],"cwd":cwd,"outputByteLimit":4096}),
        cancelled,
        false,
    );
    let Some(terminal) =
        terminal.and_then(|result| result["terminalId"].as_str().map(str::to_owned))
    else {
        return String::new();
    };
    runtime.update(session,json!({"sessionUpdate":"tool_call","toolCallId":"terminal-tool","title":"Test terminal","kind":"execute","status":"in_progress","content":[{"type":"terminal","terminalId":terminal}]}));
    let _ = call(
        runtime,
        session,
        "terminal/wait_for_exit",
        json!({"terminalId":terminal}),
        cancelled,
        false,
    );
    let result = call(
        runtime,
        session,
        "terminal/output",
        json!({"terminalId":terminal}),
        cancelled,
        false,
    );
    let _ = call(
        runtime,
        session,
        "terminal/release",
        json!({"terminalId":terminal}),
        cancelled,
        false,
    );
    let output = format!(
        "Read: {}; terminal: {}",
        read.unwrap_or(Value::Null),
        result.unwrap_or(Value::Null)
    );
    emit(runtime, session, &output);
    output
}

fn editor_review_turn(runtime: &Runtime, session: &str, cancelled: &AtomicBool) -> String {
    let cwd = runtime.sessions.lock().unwrap()[session].cwd.clone();
    let path = std::path::Path::new(&cwd).join("agent-review.txt");
    let content =
        "one\nagent two\nthree\nfour\nfive\nsix\nseven\neight\nnine\nagent ten\neleven\ntwelve\n";
    let result = call(
        runtime,
        session,
        "fs/write_text_file",
        json!({"path":path,"content":content}),
        cancelled,
        false,
    );
    let output = format!("Editor write: {}", result.unwrap_or(Value::Null));
    emit(runtime, session, &output);
    output
}

fn ordinary_turn(runtime: &Runtime, session: &str, text: &str, cancelled: &AtomicBool) -> String {
    if text == "silent-no-output" {
        while !cancelled.load(Ordering::SeqCst) {
            runtime.update(
                session,
                json!({"sessionUpdate":"usage_update","used":1,"size":1000}),
            );
            thread::sleep(Duration::from_millis(50));
        }
        return String::new();
    }
    if matches!(text, "silent" | "ignore-cancel") {
        while text == "ignore-cancel" || !cancelled.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
        }
        return String::new();
    }
    if text == "quiet-then-resume" {
        std::fs::write(
            runtime
                .settings
                .directory
                .join(format!("quiet-then-resume-{session}.started")),
            b"started",
        )
        .expect("fake quiet interval marker");
        let release = runtime
            .settings
            .directory
            .join(format!("quiet-then-resume-{session}.release"));
        while !release.exists() && !cancelled.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
        }
        if cancelled.load(Ordering::SeqCst) {
            return String::new();
        }
        let output = "Resumed after the quiet interval";
        emit(runtime, session, output);
        return output.into();
    }
    let chunks = if text == "slow" {
        (0..30).map(|number| format!("chunk-{number} ")).collect()
    } else {
        vec!["Echo: ".into(), text.into()]
    };
    let mut output = String::new();
    for chunk in chunks {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        emit(runtime, session, &chunk);
        output.push_str(&chunk);
        thread::sleep(Duration::from_millis(if text == "slow" { 100 } else { 20 }));
    }
    output
}

fn review_turn(text: &str) -> Option<String> {
    if !text.starts_with("Run the contract-drift review before any other review.") {
        return None;
    }
    let mut findings = Vec::new();
    let mut current: Option<(String, String, String, String)> = None;
    for line in text.lines() {
        if let Some((identity, kind)) = line.rsplit_once(" [")
            && let Some(kind) = kind.strip_suffix(']')
            && let Some((path, name)) = identity.rsplit_once("::")
        {
            if let Some(item) = current.take() {
                findings.push(review_finding(item));
            }
            current = Some((path.into(), name.into(), kind.into(), String::new()));
        } else if let Some((_, _, _, patch)) = current.as_mut() {
            patch.push_str(line);
            patch.push('\n');
        }
    }
    if let Some(item) = current {
        findings.push(review_finding(item));
    }
    serde_json::to_string(&findings).ok()
}

fn review_finding(item: (String, String, String, String)) -> Value {
    let (path, name, kind, patch) = item;
    let label = if patch.contains("todo!()") || patch.contains("unimplemented!()") {
        "incomplete"
    } else if patch.contains(".unwrap_or_default()") {
        "silent_fallback"
    } else {
        "equivalent"
    };
    json!({"path":path,"name":name,"kind":kind,"label":label})
}

fn execute_turn(
    runtime: &Arc<Runtime>,
    session: &str,
    text: &str,
    cancelled: &AtomicBool,
) -> String {
    if text == "separate-message-ids" {
        for (id, chunk) in [
            ("first", "Failed to authenticate: OAuth session expired"),
            ("second", "No response requested."),
        ] {
            runtime.update(
                session,
                json!({"sessionUpdate":"agent_message_chunk","messageId":id,"content":{"type":"text","text":chunk}}),
            );
        }
        return "Failed to authenticate: OAuth session expired\n\nNo response requested.".into();
    }
    if let Some(model) = match text {
        "reject-model-as-reply" => Some("model-a"),
        "reject-other-model-as-reply" => Some("model-z"),
        _ => None,
    } {
        // The provider's refusal of a model, written as an ordinary reply that ends the turn.
        let reply = format!(
            "There's an issue with the selected model ({model}). It may not exist or you may not have access to it. Run /model to pick a different model."
        );
        emit(runtime, session, &reply);
        return reply;
    }
    if text == "reject-model-after-a-message" {
        // The refusal sentence follows an earlier reply of the same turn.
        let reply = "There's an issue with the selected model (model-a). It may not exist or you may not have access to it.";
        for (id, chunk) in [("first", "Checking the account."), ("second", reply)] {
            runtime.update(
                session,
                json!({"sessionUpdate":"agent_message_chunk","messageId":id,"content":{"type":"text","text":chunk}}),
            );
        }
        return format!("Checking the account.\n\n{reply}");
    }
    if let Some(output) = review_turn(text) {
        emit(runtime, session, &output);
        return output;
    }
    let turn = runtime.sessions.lock().unwrap()[session].history.len();
    if let Some(script) = crate::subagents::script(text, session, turn) {
        if !runtime.native_subagents.load(Ordering::SeqCst) {
            let output = "The client did not advertise native subagents.".to_owned();
            emit(runtime, session, &output);
            return output;
        }
        return crate::subagents::emit(runtime, session, script);
    }
    let saved = runtime.sessions.lock().unwrap()[session].clone();
    if let Some(output) = crate::skills::turn(runtime, &saved, text) {
        emit(runtime, session, &output);
        return output;
    }
    if let Some(output) = crate::mcp::task_turn(runtime, session, text)
        .or_else(|| crate::mcp::peer_turn(runtime, session, text))
    {
        emit(runtime, session, &output);
        return output;
    }
    if let Some(mode) = text.strip_prefix("switch-mode:") {
        runtime.switch_mode(session, mode);
        let output = format!("Switched to {mode}");
        emit(runtime, session, &output);
        return output;
    }
    if text.starts_with("permission") {
        permission_turn(runtime, session, text, cancelled)
    } else {
        match text {
            "elicitation" => elicitation_turn(runtime, session, cancelled),
            "terminal-missing" => missing_terminal_turn(runtime, session, cancelled),
            "tools" => tools_turn(runtime, session, cancelled),
            "editor-review" => editor_review_turn(runtime, session, cancelled),
            _ => ordinary_turn(runtime, session, text, cancelled),
        }
    }
}

pub fn run(
    runtime: Arc<Runtime>,
    id: Value,
    session: String,
    text: String,
    cancelled: Arc<AtomicBool>,
) {
    if text == "die" {
        std::process::exit(17);
    }
    if text == "silent-no-output" {
        std::fs::write(
            runtime
                .settings
                .directory
                .join(format!("silent-no-output-{session}.started")),
            b"started",
        )
        .expect("fake silence marker");
        // Mirrors a measured wedged adapter: catalogue and accounting noise
        // arrive even though the requested turn has produced no output.
        runtime.update(
            &session,
            json!({"sessionUpdate":"available_commands_update","availableCommands":[]}),
        );
        runtime.update(
            &session,
            json!({"sessionUpdate":"usage_update","used":1,"size":1000}),
        );
    }
    if text != "silent-no-output" {
        runtime.update(&session,json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Checking the test request."}}));
    }
    let output = execute_turn(&runtime, &session, &text, &cancelled);
    let mut sessions = runtime.sessions.lock().unwrap();
    if let Some(saved) = sessions.get_mut(&session) {
        saved.history.push((text, output));
        if saved.save(&runtime.settings).is_err() {
            runtime.active.lock().unwrap().remove(&session);
            runtime.error(id, -32603, "Could not persist fake transcript");
            return;
        }
    }
    drop(sessions);
    runtime.update(
        &session,
        json!({"sessionUpdate":"usage_update","used":42,"size":8192}),
    );
    runtime.active.lock().unwrap().remove(&session);
    runtime.answer(
        id,
        json!({"stopReason":if cancelled.load(Ordering::SeqCst){"cancelled"}else{"end_turn"}}),
    );
}
