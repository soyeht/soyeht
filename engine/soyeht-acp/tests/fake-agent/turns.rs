use crate::{Message, Runtime};
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
    let mut output = String::new();
    runtime.update(&session,json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Checking the test request."}}));
    if matches!(text.as_str(), "silent" | "ignore-cancel") {
        while text == "ignore-cancel" || !cancelled.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
        }
    } else if text.starts_with("permission") {
        if matches!(text.as_str(), "permission-delayed" | "permission-overflow") {
            thread::sleep(Duration::from_millis(500));
        }
        if text == "permission-overflow" {
            let flood = Arc::clone(&runtime);
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
        let edit = text == "permission-edit";
        let result = call(
            &runtime,
            &session,
            "session/request_permission",
            json!({"toolCall":{"toolCallId":"permission-tool","title":"Run test operation","kind":if edit{"edit"}else{"execute"},"status":"pending"},"options":[{"optionId":"once","kind":"allow_once","name":"Allow once"},{"optionId":"always","kind":"allow_always","name":"Always allow"},{"optionId":"deny","kind":"reject_once","name":"Deny"}]}),
            &cancelled,
            text == "permission-null",
        );
        if let Some(result) = result {
            output = format!("Permission result: {}", result["outcome"]);
            emit(&runtime, &session, &output);
        }
    } else if text == "elicitation" {
        let result = call(
            &runtime,
            &session,
            "elicitation/create",
            json!({"mode":"form","message":"Choose a test answer","requestedSchema":{"type":"object","properties":{"answer":{"type":"string","title":"Answer"}},"required":["answer"]}}),
            &cancelled,
            false,
        );
        if let Some(result) = result {
            output = format!("Form result: {result}");
            emit(&runtime, &session, &output);
        }
    } else if text == "terminal-missing" {
        let result = call(
            &runtime,
            &session,
            "terminal/create",
            json!({"command":runtime.settings.directory.join("nonexistent-command")}),
            &cancelled,
            false,
        );
        output = format!("Terminal creation: {}", result.unwrap_or(Value::Null));
        emit(&runtime, &session, &output);
    } else if text == "tools" {
        let cwd = runtime.sessions.lock().unwrap()[&session].cwd.clone();
        let path = std::path::Path::new(&cwd).join("fake-acp-note.txt");
        let _ = call(
            &runtime,
            &session,
            "fs/write_text_file",
            json!({"path":path,"content":"test data\n"}),
            &cancelled,
            false,
        );
        let read = call(
            &runtime,
            &session,
            "fs/read_text_file",
            json!({"path":path}),
            &cancelled,
            false,
        );
        let terminal = call(
            &runtime,
            &session,
            "terminal/create",
            json!({"command":"/bin/sh","args":["-c","printf fake-terminal"],"cwd":cwd,"outputByteLimit":4096}),
            &cancelled,
            false,
        );
        if let Some(terminal) = terminal.and_then(|r| r["terminalId"].as_str().map(str::to_owned)) {
            runtime.update(&session,json!({"sessionUpdate":"tool_call","toolCallId":"terminal-tool","title":"Test terminal","kind":"execute","status":"in_progress","content":[{"type":"terminal","terminalId":terminal}]}));
            let _ = call(
                &runtime,
                &session,
                "terminal/wait_for_exit",
                json!({"terminalId":terminal}),
                &cancelled,
                false,
            );
            let result = call(
                &runtime,
                &session,
                "terminal/output",
                json!({"terminalId":terminal}),
                &cancelled,
                false,
            );
            let _ = call(
                &runtime,
                &session,
                "terminal/release",
                json!({"terminalId":terminal}),
                &cancelled,
                false,
            );
            output = format!(
                "Read: {}; terminal: {}",
                read.unwrap_or(Value::Null),
                result.unwrap_or(Value::Null)
            );
            emit(&runtime, &session, &output);
        }
    } else {
        let chunks = if text == "slow" {
            (0..30).map(|n| format!("chunk-{n} ")).collect::<Vec<_>>()
        } else {
            vec!["Echo: ".into(), text.clone()]
        };
        for chunk in chunks {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            emit(&runtime, &session, &chunk);
            output.push_str(&chunk);
            thread::sleep(Duration::from_millis(if text == "slow" { 100 } else { 20 }));
        }
    }
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
