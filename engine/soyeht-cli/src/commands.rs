use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use soyeht_cli::{Client, Error, Result};
use soyeht_protocol::*;
use std::path::PathBuf;
use uuid::Uuid;

pub const USAGE: &str = "soyeht --profile dev [--data-dir PATH] COMMAND\n\
status\n\
pane create --shell|--agent HARNESS [--cwd PATH] [--model MODEL] [--effort EFFORT] [--permission ask|accept_edits|allow_all]\n\
pane list | pane close ID | pane prompt ID TEXT | pane cancel ID\n\
pane permission ID allow|deny [--request REQUEST_ID]\n\
pane config ID KEY VALUE | pane yolo ID on|off | pane auth ID METHOD\n\
pane transcript ID [--before CURSOR] [--limit COUNT]\n\
pane queue ID [--cursor CURSOR] | pane queue-resume ID | pane queue-remove ID PROMPT_ID\n\
harness list [--cursor CURSOR] | model list HARNESS [--pane ID] [--cursor CURSOR]\n\
device pair-start | device list\n\
Mutating commands accept --idempotency-key KEY for safe retries.";

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
    // Read the optional suffix without treating literal prompt text or a
    // configuration value as a command-line flag.
    if args.len() >= 2 && args[args.len() - 2] == "--idempotency-key" {
        key = args
            .last()
            .ok_or_else(|| invalid("Missing idempotency key"))?
            .to_string();
        args.truncate(args.len() - 2);
    }
    match args.as_slice() {
        ["status"] => rpc::<EngineStatusResult>(client, "engine.status", json!({})),
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
        ["pane", "config", id, config, value] => {
            let snapshot = client.snapshot()?;
            let boolean = snapshot
                .sections
                .get("panes")
                .into_iter()
                .flatten()
                .find(|pane| pane["paneId"] == *id)
                .and_then(|pane| pane["agent"]["options"].as_array())
                .and_then(|options| options.iter().find(|option| option["id"] == *config))
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
                json!({"paneId":id,"configId":config,"value":value,"idempotencyKey":key}),
            )
        }
        ["pane", "yolo", id, on] if matches!(*on, "on" | "off") => rpc::<PaneYoloResult>(
            client,
            "pane.yolo",
            json!({"paneId":id,"on":*on=="on","idempotencyKey":key}),
        ),
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
        _ => Err(invalid(USAGE)),
    }
}

fn create(client: &mut Client, args: &[&str], key: &str) -> Result<Value> {
    let (mode, harness, rest) = match args {
        ["--shell", rest @ ..] => ("terminal", None, rest),
        ["--agent", harness, rest @ ..] => ("agent", Some(*harness), rest),
        _ => return Err(invalid("Choose --shell or --agent HARNESS")),
    };
    let allowed = if harness.is_some() {
        &[
            ("--cwd", "cwd"),
            ("--model", "model"),
            ("--effort", "effort"),
            ("--permission", "permission"),
        ][..]
    } else {
        &[("--cwd", "cwd")][..]
    };
    let mut params = options(rest, allowed)?;
    if params
        .get("permission")
        .is_some_and(|value| !matches!(value.as_str(), Some("ask" | "accept_edits" | "allow_all")))
    {
        return Err(invalid("Permission must be ask, accept_edits or allow_all"));
    }
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
