//! Subagent turns in the shapes the pinned adapters report them natively
//! (ACP subagent draft): `subagent_spawned` and `subagent_state_update` in the
//! parent session, each subagent's activity under its own session ID.
use crate::Runtime;
use serde_json::{Value, json};

/// One update in the order the adapter sends it: (session, update).
type Script = Vec<(String, Value)>;

pub fn spawned(id: &str, name: &str, task: &str) -> Value {
    json!({"sessionUpdate":"subagent_spawned","subagentSessionId":id,"name":name,"task":task,
        "capabilities":{}})
}

pub fn state(id: &str, state: &str) -> Value {
    json!({"sessionUpdate":"subagent_state_update","subagentSessionId":id,"state":state})
}

fn message(text: &str) -> Value {
    json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}})
}

fn tool(id: &str, title: &str, status: &str) -> Value {
    json!({"sessionUpdate":"tool_call","toolCallId":id,"title":title,"kind":"execute",
        "status":status})
}

fn tool_done(id: &str, status: &str, meta: Value) -> Value {
    json!({"sessionUpdate":"tool_call_update","toolCallId":id,"status":status,"_meta":meta})
}

/// codex-acp's collaboration `wait` call, as captured from codex-acp 1.11.0:
/// the parent waits with an empty `receiverThreadIds`.
fn wait(session: &str, id: &str, update: &str, status: &str) -> Value {
    let collaboration = json!({"tool":"wait","senderThreadId":session,"receiverThreadIds":[]});
    json!({"sessionUpdate":update,"toolCallId":id,"kind":"other","title":"wait","status":status,
        "rawInput":{"prompt":null,"senderThreadId":session,"receiverThreadIds":[],
            "agentsStates":{},"model":null,"reasoningEffort":null,"status":status},
        "_meta":{"codex":{"collaboration":collaboration}}})
}

/// Claude Code: children are tagged with the spawning Agent call, whose
/// completion reports the subagent ID and the model it resolved.
fn claude(session: &str, turn: usize) -> Script {
    let child = format!("{session}-claude-{turn}");
    let agent_call = format!("toolu-agent-{turn}");
    let tagged = json!({"claudeCode":{"toolName":"Bash","parentToolUseId":agent_call}});
    vec![
        (session.into(), message("I'll delegate the search.")),
        (
            session.into(),
            spawned(
                &child,
                "Find the config",
                "Search the repository for the config file",
            ),
        ),
        (
            child.clone(),
            tool("toolu-child-ls", "ls config", "pending"),
        ),
        (
            child.clone(),
            tool_done("toolu-child-ls", "completed", tagged),
        ),
        (child.clone(), message("The config is config/app.toml.")),
        (session.into(), state(&child, "completed")),
        (
            session.into(),
            tool_done(
                &agent_call,
                "completed",
                json!({"claudeCode":{"toolName":"Agent","toolResponse":{"status":"completed",
                    "agentId":child,"resolvedModel":"fake-claude-model"}}}),
            ),
        ),
        (session.into(), message("The config is in config/app.toml.")),
    ]
}

/// Codex: names come from the agent path, children report their own usage
/// and thread status, and the parent waits with a `wait` collaboration call.
fn codex(session: &str, turn: usize) -> Script {
    let child = format!("{session}-codex-{turn}");
    vec![
        (
            session.into(),
            spawned(&child, "List files", "Delegated task for List files"),
        ),
        (
            child.clone(),
            json!({"sessionUpdate":"session_info_update","_meta":{"codex":{"threadStatus":{"type":"active"}}}}),
        ),
        (
            child.clone(),
            tool("exec-child-ls", "List files", "in_progress"),
        ),
        (
            child.clone(),
            tool_done("exec-child-ls", "completed", json!({})),
        ),
        (
            child.clone(),
            json!({"sessionUpdate":"usage_update","used":7,"size":99}),
        ),
        (child.clone(), message("Two files.")),
        (
            session.into(),
            wait(session, "call-wait", "tool_call", "in_progress"),
        ),
        (session.into(), state(&child, "completed")),
        (
            session.into(),
            wait(session, "call-wait", "tool_call_update", "completed"),
        ),
        // Codex waits once more after its last subagent finished.
        (
            session.into(),
            wait(session, "call-wait-2", "tool_call", "in_progress"),
        ),
        (
            session.into(),
            wait(session, "call-wait-2", "tool_call_update", "completed"),
        ),
        (session.into(), message("Subagent found two files.")),
    ]
}

/// Two subagents whose activity interleaves; the first finishes last.
fn parallel(session: &str, turn: usize) -> Script {
    let first = format!("{session}-first-{turn}");
    let second = format!("{session}-second-{turn}");
    vec![
        (session.into(), spawned(&first, "First", "First task")),
        (session.into(), spawned(&second, "Second", "Second task")),
        (
            first.clone(),
            tool("first-tool", "first step", "in_progress"),
        ),
        (
            second.clone(),
            tool("second-tool", "second step", "in_progress"),
        ),
        (
            second.clone(),
            tool_done("second-tool", "completed", json!({})),
        ),
        (second.clone(), message("Second done.")),
        (session.into(), state(&second, "completed")),
        (
            first.clone(),
            tool_done("first-tool", "completed", json!({})),
        ),
        (first.clone(), message("First done.")),
        (session.into(), state(&first, "completed")),
        (session.into(), message("Both finished.")),
    ]
}

fn failure(session: &str, turn: usize) -> Script {
    let child = format!("{session}-failing-{turn}");
    vec![
        (
            session.into(),
            spawned(&child, "Doomed", "Run the broken step"),
        ),
        (
            child.clone(),
            tool("broken-tool", "broken step", "in_progress"),
        ),
        (child.clone(), tool_done("broken-tool", "failed", json!({}))),
        (session.into(), state(&child, "failed")),
        (session.into(), message("The subagent failed.")),
    ]
}

/// The updates a subagent prompt produces as the `turn`-th turn of `session`.
pub fn script(prompt: &str, session: &str, turn: usize) -> Option<Script> {
    Some(match prompt {
        "subagents-claude" => claude(session, turn),
        "subagents-codex" => codex(session, turn),
        "subagents-parallel" => parallel(session, turn),
        "subagents-failure" => failure(session, turn),
        _ => return None,
    })
}

/// Sends a script live or as a native replay, and returns the parent's text.
pub fn emit(runtime: &Runtime, parent: &str, script: Script) -> String {
    let mut output = String::new();
    for (session, update) in script {
        if session == parent && update["sessionUpdate"] == "agent_message_chunk" {
            output.push_str(update["content"]["text"].as_str().unwrap_or_default());
        }
        runtime.update(&session, update);
    }
    output
}

/// Real adapters report subagents natively only to clients that advertise
/// the draft capability or its AIR `_meta` equivalent.
pub fn client_supports(capabilities: &Value) -> bool {
    capabilities["subagents"].is_object()
        || capabilities["_meta"]["jetbrains"]["air"]["capabilities"]
            .as_array()
            .is_some_and(|names| names.iter().any(|name| name == "nativeSubagentSessions"))
}
