use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use soyeht_cli::{Client, Error, Result};
use soyeht_protocol::*;
use std::{
    io::{self, Read},
    path::PathBuf,
};
use uuid::Uuid;

use crate::{editor_commands, task_commands};

pub const USAGE: &str = "soyeht --profile dev [--data-dir PATH] [--bound] COMMAND\n\
status\n\
pane create --shell|--agent HARNESS|--editor FILE [--cwd PATH] [--model MODEL] [--effort EFFORT] [--permission-mode MODE]\n\
pane list | pane close ID | pane prompt ID TEXT | pane cancel ID\n\
pane permission ID allow|deny [--request REQUEST_ID]\n\
pane config ID KEY VALUE | pane mode ID MODE | pane auth ID METHOD\n\
pane transcript ID [--before CURSOR] [--limit COUNT]\n\
pane queue ID [--cursor CURSOR] | pane queue-resume ID | pane queue-remove ID PROMPT_ID\n\
harness list [--cursor CURSOR] | model list HARNESS [--pane ID] [--cursor CURSOR]\n\
device pair-start | device list\n\
task create WORKSPACE TITLE --must-not-change TEXT [OPTIONS]\n\
task next WORKSPACE PANE GENERATION | task list WORKSPACE [OPTIONS] | task show WORKSPACE TASK\n\
task dispatch WORKSPACE TASK [OPTIONS] | task update WORKSPACE TASK VERSION [OPTIONS]\n\
task comment WORKSPACE TASK TEXT | task subitem add WORKSPACE TASK TEXT [OPTIONS]\n\
task subitem done WORKSPACE TASK SUBITEM | task block WORKSPACE TASK KIND REASON [OPTIONS]\n\
task release WORKSPACE TASK [OPTIONS] | task request-review WORKSPACE TASK [OPTIONS]\n\
task complete WORKSPACE TASK SUMMARY [OPTIONS] | task move WORKSPACE TASK STATE REASON\n\
task unblock WORKSPACE TASK [OPTIONS] | task worktree-clean WORKSPACE TASK\n\
loop set PANE PROMPT INTERVAL_SECONDS MAX_ROUNDS STOP_WHEN ENABLED\n\
file open WORKSPACE PATH [OPTIONS] | file read FILE [OPTIONS] | file write FILE REVISION START_LINE START_COLUMN END_LINE END_COLUMN TEXT\n\
file history FILE [OPTIONS] | file revert FILE VERSION REVISION | file search WORKSPACE QUERY [OPTIONS]\n\
file symbols FILE REVISION | file diff FILE REVISION | usage list | usage refresh [PROVIDER]\n\
Task create options: --description, --how-to-verify, --harness, --model, --effort,\n\
  --priority, --create-worktree true|false, --permission-mode, --dependencies WS-1,WS-2.\n\
Task update uses the same editable text, priority and dependency options.\n\
Task complete accepts --contract-changes JSON and --out-of-scope-edits JSON arrays.\n\
Executor commands accept --pane ID --generation N --attempt N together.\n\
Use --bound to read SOYEHT_MCP_BINDING from the pane environment for agent-only calls.\n\
Mutating commands accept --idempotency-key KEY as the final option for safe retries.";

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArguments(message.into())
}
fn rpc<T: DeserializeOwned + Serialize>(
    client: &mut Client,
    method: &str,
    params: Value,
) -> Result<Value> {
    Ok(serde_json::to_value(client.request::<T>(method, params)?)?)
}
fn options(rest: &[&str], allowed: &[(&str, &str)]) -> Result<Value> {
    let mut value = json!({});
    let mut args = rest.iter();
    while let Some(flag) = args.next() {
        let key = allowed
            .iter()
            .find(|(name, _)| name == flag)
            .map(|(_, key)| *key)
            .ok_or_else(|| invalid(format!("Unknown option: {flag}")))?;
        if value.get(key).is_some() {
            return Err(invalid(format!("Repeated option: {flag}")));
        }
        let text = args
            .next()
            .ok_or_else(|| invalid(format!("Missing value for {flag}")))?;
        value[key] = if key == "limit" {
            json!(
                text.parse::<u32>()
                    .map_err(|_| invalid("Limit must be an integer"))?
            )
        } else {
            json!(text)
        };
    }
    Ok(value)
}

pub fn execute(client: &mut Client, args: &[String]) -> Result<Value> {
    let mut args: Vec<_> = args.iter().map(String::as_str).collect();
    let mut key = Uuid::new_v4().to_string();
    // Read the optional suffix without treating prompt text or a value as a flag.
    if args.len() >= 2 && args[args.len() - 2] == "--idempotency-key" {
        key = args
            .last()
            .ok_or_else(|| invalid("Missing idempotency key"))?
            .to_string();
        args.truncate(args.len() - 2);
    }
    match args.as_slice() {
        ["status"] => rpc::<EngineStatusResult>(client, "engine.status", json!({})),
        ["__experiment-hook", capability] => experiment_hook(client, capability),
        ["pane", "list"] => {
            Ok(json!({"items":client.snapshot()?.sections.remove("panes").unwrap_or_default()}))
        }
        ["pane", "create", rest @ ..] => create(client, rest, &key),
        ["pane", "close", id] => rpc::<PaneCloseResult>(
            client,
            "pane.close",
            json!({"paneId":id,"idempotencyKey":key}),
        ),
        ["pane", "prompt", id, text] => rpc::<PanePromptResult>(
            client,
            "pane.prompt",
            json!({"paneId":id,"text":text,"idempotencyKey":key}),
        ),
        ["pane", "cancel", id] => rpc::<PaneCancelResult>(
            client,
            "pane.cancel",
            json!({"paneId":id,"idempotencyKey":key}),
        ),
        ["pane", "permission", id, choice, rest @ ..] if matches!(*choice, "allow" | "deny") => {
            permission(client, id, choice, rest, &key)
        }
        ["pane", "config", id, config, value] => set_config(client, id, config, value, &key),
        ["pane", "mode", id, mode] => set_mode(client, id, mode, &key),
        ["pane", "auth", id, method] => rpc::<PaneAuthStartResult>(
            client,
            "pane.auth.start",
            json!({"paneId":id,"authMethodId":method,"idempotencyKey":key}),
        ),
        ["pane", "transcript", id, rest @ ..] => {
            let mut params = options(rest, &[("--before", "before"), ("--limit", "limit")])?;
            params["paneId"] = json!(id);
            rpc::<PaneTranscriptResult>(client, "pane.transcript", params)
        }
        ["pane", "queue", id, rest @ ..] => {
            let mut params = options(rest, &[("--cursor", "cursor"), ("--limit", "limit")])?;
            params["paneId"] = json!(id);
            rpc::<PaneQueueListResult>(client, "pane.queue.list", params)
        }
        ["pane", "queue-resume", id] => rpc::<PaneQueueResumeResult>(
            client,
            "pane.queue.resume",
            json!({"paneId":id,"idempotencyKey":key}),
        ),
        ["pane", "queue-remove", id, prompt] => rpc::<PaneQueueRemoveResult>(
            client,
            "pane.queue.remove",
            json!({"paneId":id,"promptId":prompt,"idempotencyKey":key}),
        ),
        ["harness", "list", rest @ ..] => rpc::<HarnessListResult>(
            client,
            "harness.list",
            options(rest, &[("--cursor", "cursor"), ("--limit", "limit")])?,
        ),
        ["model", "list", harness, rest @ ..] => {
            let mut params = options(
                rest,
                &[
                    ("--pane", "paneId"),
                    ("--cursor", "cursor"),
                    ("--limit", "limit"),
                ],
            )?;
            params["harness"] = json!(harness);
            rpc::<ModelListResult>(client, "model.list", params)
        }
        ["device", "list"] => rpc::<DeviceListResult>(client, "device.list", json!({})),
        ["device", "pair-start"] => {
            rpc::<DevicePairStartResult>(client, "device.pair.start", json!({"idempotencyKey":key}))
        }
        ["file", rest @ ..] => editor_commands::execute_file(client, rest, &key),
        ["usage", rest @ ..] => editor_commands::execute_usage(client, rest, &key),
        ["task", rest @ ..] => task_commands::execute_task(client, rest, &key),
        ["loop", rest @ ..] => task_commands::execute_loop(client, rest, &key),
        _ => Err(invalid(USAGE)),
    }
}

fn experiment_hook(client: &mut Client, capability: &str) -> Result<Value> {
    const MAX_HOOK_INPUT_BYTES: u64 = 64 * 1024;
    let mut bytes = Vec::new();
    io::stdin()
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_HOOK_INPUT_BYTES {
        return Err(invalid("Hook input exceeds its bound"));
    }
    let input: Value = serde_json::from_slice(&bytes)?;
    let event = input["hook_event_name"]
        .as_str()
        .ok_or_else(|| invalid("Hook input has no event name"))?;
    let event: ExperimentHookEvent =
        serde_json::from_value(json!(event)).map_err(|_| invalid("Unsupported hook event"))?;
    let _: PaneExperimentHookResult = client.request(
        "pane.experiment.hook",
        serde_json::to_value(PaneExperimentHookParams {
            event,
            capability: capability.into(),
            idempotency_key: Uuid::new_v4().to_string(),
        })?,
    )?;
    // Command hooks treat a JSON object on stdout as a control response. An
    // empty object records the event without changing or blocking the turn.
    Ok(json!({}))
}

/// One of the options a pane's agent announced, from a fresh snapshot.
fn agent_option(
    client: &mut Client,
    pane: &str,
    matches: impl Fn(&Value) -> bool,
) -> Result<Option<Value>> {
    let snapshot = client.snapshot()?;
    Ok(snapshot
        .sections
        .get("panes")
        .into_iter()
        .flatten()
        .find(|item| item["paneId"] == pane)
        .and_then(|item| item["agent"]["options"].as_array())
        .and_then(|options| options.iter().find(|option| matches(option)).cloned()))
}

fn set_config(
    client: &mut Client,
    pane: &str,
    config: &str,
    value: &str,
    key: &str,
) -> Result<Value> {
    let boolean = agent_option(client, pane, |option| option["id"] == config)?
        .is_some_and(|option| option["type"] == "boolean");
    let value = if boolean {
        json!(
            value
                .parse::<bool>()
                .map_err(|_| invalid("This option requires true or false"))?
        )
    } else {
        json!(value)
    };
    rpc::<PaneConfigSetResult>(
        client,
        "pane.config.set",
        json!({"paneId":pane,"configId":config,"value":value,"idempotencyKey":key}),
    )
}

/// Selects one of the harness's own permission modes by its native ID.
fn set_mode(client: &mut Client, pane: &str, mode: &str, key: &str) -> Result<Value> {
    let option = agent_option(client, pane, |option| {
        serde_json::from_value::<ConfigOption>(option.clone())
            .is_ok_and(|option| option.is_permission_mode())
    })?
    .ok_or_else(|| invalid("This pane announces no permission modes"))?;
    let config = option["id"]
        .as_str()
        .ok_or_else(|| invalid("The permission mode selector has no ID"))?;
    rpc::<PaneConfigSetResult>(
        client,
        "pane.config.set",
        json!({"paneId":pane,"configId":config,"value":mode,"idempotencyKey":key}),
    )
}

fn create(client: &mut Client, args: &[&str], key: &str) -> Result<Value> {
    let (mode, harness, editor, rest) = match args {
        ["--shell", rest @ ..] => ("terminal", None, None, rest),
        ["--agent", harness, rest @ ..] => ("agent", Some(*harness), None, rest),
        ["--editor", path, rest @ ..] => ("editor", None, Some(*path), rest),
        _ => return Err(invalid("Choose --shell, --agent HARNESS or --editor FILE")),
    };
    let allowed = if harness.is_some() {
        &[
            ("--cwd", "cwd"),
            ("--model", "model"),
            ("--effort", "effort"),
            ("--permission-mode", "permissionMode"),
        ][..]
    } else {
        &[("--cwd", "cwd")][..]
    };
    let mut params = options(rest, allowed)?;
    let cwd = params["cwd"]
        .as_str()
        .map(PathBuf::from)
        .map_or_else(std::env::current_dir, Ok)?
        .canonicalize()?;
    let path = cwd
        .to_str()
        .ok_or_else(|| invalid("Directory must be valid UTF-8"))?;
    let workspaces: WorkspaceListResult = client.request("workspace.list", json!({}))?;
    let workspace = if let Some(workspace) = workspaces.items.into_iter().find(|w| w.path == path) {
        workspace
    } else {
        let created: WorkspaceCreateResult = client.request("workspace.create",json!({"name":cwd.file_name().and_then(|v| v.to_str()).unwrap_or("Workspace"),"path":path,"idempotencyKey":Uuid::new_v4().to_string()}))?;
        created.workspace
    };
    params["workspaceId"] = json!(workspace.workspace_id);
    params["mode"] = json!(mode);
    params["cwd"] = json!(path);
    params["idempotencyKey"] = json!(key);
    if let Some(harness) = harness {
        params["harness"] = json!(harness);
    }
    if let Some(path) = editor {
        params["path"] = json!(path);
    }
    rpc::<PaneCreateResult>(client, "pane.create", params)
}

fn permission(
    client: &mut Client,
    pane: &str,
    choice: &str,
    rest: &[&str],
    key: &str,
) -> Result<Value> {
    let options = options(rest, &[("--request", "requestId")])?;
    let snapshot = client.snapshot()?;
    let requests: Vec<_> = snapshot
        .sections
        .get("requests")
        .into_iter()
        .flatten()
        .filter(|entry| entry["kind"] == "permission" && entry["request"]["paneId"] == pane)
        .map(|entry| &entry["request"])
        .filter(|request| {
            options
                .get("requestId")
                .is_none_or(|id| request["requestId"] == *id)
        })
        .collect();
    if requests.len() != 1 {
        return Err(invalid(
            "Select exactly one pending permission with --request REQUEST_ID",
        ));
    }
    let request = requests[0];
    let kind = if choice == "allow" {
        "allow_once"
    } else {
        "reject_once"
    };
    let option = request["options"]
        .as_array()
        .and_then(|values| values.iter().find(|option| option["kind"] == kind))
        .ok_or_else(|| invalid("The agent does not offer that one-time permission choice"))?;
    rpc::<PanePermissionAnswerResult>(
        client,
        "pane.permission.answer",
        json!({"paneId":pane,"requestId":request["requestId"],"optionId":option["optionId"],"idempotencyKey":key}),
    )
}
