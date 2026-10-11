use crate::{Runtime, error::Result};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

const REQUIRED_AGENT_TOOLS: [&str; 8] = [
    "task_next",
    "task_subitem_add",
    "task_subitem_done",
    "task_comment",
    "task_block",
    "task_request_review",
    "task_complete",
    "task_release",
];

const REQUIRED_CHIEF_TOOLS: [&str; 8] = [
    "task_create",
    "task_show",
    "task_list",
    "task_update",
    "task_comment",
    "task_dispatch",
    "task_unblock",
    "loop_set",
];

pub struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    pub fn from_servers(servers: &[Value]) -> Result<Option<Self>> {
        let Some(server) = servers
            .iter()
            .find(|server| server["name"] == "soyeht-workspace")
        else {
            return Ok(None);
        };
        let command = server["command"]
            .as_str()
            .filter(|value| Path::new(value).is_absolute());
        let args = string_array(&server["args"]);
        let env = environment(&server["env"]);
        let (Some(command), Some(args), Some(env)) = (command, args, env) else {
            return invalid("Invalid Soyeht MCP declaration");
        };
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io_error("MCP stdin unavailable"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io_error("MCP stdout unavailable"))?;
        let mut client = Self {
            child,
            input,
            output: BufReader::new(output),
            next_id: 0,
        };
        client.initialize()?;
        Ok(Some(client))
    }

    fn initialize(&mut self) -> Result<()> {
        let value = self.request(
            "initialize",
            json!({
                "protocolVersion":"2025-11-25",
                "capabilities":{},
                "clientInfo":{"name":"Soyeht Test Agent","version":"1"}
            }),
        )?;
        if value["protocolVersion"] != "2025-11-25" {
            return invalid("MCP protocol version mismatch");
        }
        self.notify("notifications/initialized", json!({}))?;
        let tools = self.request("tools/list", json!({}))?;
        let listed = tools["tools"]
            .as_array()
            .ok_or_else(|| io_error("MCP tool list missing"))?;
        let complete = |required: &[&str]| {
            required
                .iter()
                .all(|name| listed.iter().any(|tool| tool["name"] == *name))
        };
        if !complete(&REQUIRED_AGENT_TOOLS) && !complete(&REQUIRED_CHIEF_TOOLS) {
            return invalid("MCP task tool missing");
        }
        Ok(())
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let result = self.request("tools/call", json!({"name":name,"arguments":arguments}))?;
        if result["isError"] == true {
            return invalid("MCP task tool failed");
        }
        Ok(result["structuredContent"].clone())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            let response = self.read()?;
            if response["id"] != id {
                continue;
            }
            if response.get("error").is_some() {
                return invalid("MCP request failed");
            }
            return Ok(response
                .get("result")
                .cloned()
                .ok_or_else(|| io_error("MCP response result missing"))?);
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write(&json!({"jsonrpc":"2.0","method":method,"params":params}))
    }

    fn write(&mut self, value: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.input, value)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        Ok(())
    }

    fn read(&mut self) -> Result<Value> {
        let mut line = String::new();
        if self.output.read_line(&mut line)? == 0 {
            return invalid("MCP server closed its output");
        }
        Ok(serde_json::from_str(&line)?)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn task_turn(runtime: &Runtime, session: &str, instruction: &str) -> Option<String> {
    let mut clients = runtime.mcp_clients.lock().ok()?;
    let client = clients.get_mut(session)?;
    let result = match instruction {
        "task-claim" => client.call_tool("task_next", json!({})),
        "task-execute" => execute(client),
        "task-block" => client.call_tool(
            "task_block",
            json!({"kind":"missing_information","reason":"Need test input"}),
        ),
        "task-drift" => client.call_tool(
            "task_complete",
            json!({
                "summary":"Completed with declared drift",
                "contractChanges":[{
                    "path":"src/lib.rs",
                    "name":"run",
                    "label":"silent_fallback",
                    "contractRef":"docs/adr/0001-example.md"
                }],
                "outOfScopeEdits":[{
                    "path":"src/test_support.rs",
                    "reason":"Required test support"
                }]
            }),
        ),
        _ => return None,
    };
    Some(match result {
        Ok(value) => format!("Task operation: {value}"),
        Err(error) => format!("Task operation failed: {error}"),
    })
}

/// A peer exchange through the pane's own MCP server, like a harness calling its tools.
///
/// `list-peers` lists the other agent panes. `peer-send:<paneId>:<text>` queues `<text>` for that
/// pane. A queued message that arrives with the reply route and whose text starts with `ask:` is
/// answered with `answer: <rest>` to the route's pane. Any other peer message is only reported,
/// so two fake agents never loop.
pub fn peer_turn(runtime: &Runtime, session: &str, instruction: &str) -> Option<String> {
    let mut clients = runtime.mcp_clients.lock().ok()?;
    let client = clients.get_mut(session)?;
    let message = |client: &mut Client, pane: &str, text: &str| {
        client.call_tool("message_agent", json!({"targetPaneId":pane,"text":text}))
    };
    let outcome = if instruction == "list-peers" {
        client.call_tool("list_agents", json!({})).map(|peers| {
            let names = peers["items"].as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|peer| peer["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            format!("Peers: {}", names.unwrap_or_default())
        })
    } else if let Some((pane, text)) = instruction
        .strip_prefix("peer-send:")
        .and_then(|rest| rest.split_once(':'))
    {
        message(client, pane, text).map(|_| format!("Sent to {pane}: {text}"))
    } else if let Some((sender, route, body)) = incoming_peer(instruction) {
        match body.strip_prefix("ask:") {
            Some(question) => message(client, route, &format!("answer:{question}"))
                .map(|_| format!("Answered {sender}:{question}")),
            None => Ok(format!("Received from {sender}: {body}")),
        }
    } else {
        return None;
    };
    Some(match outcome {
        Ok(value) => value,
        Err(error) => format!("Peer operation failed: {error}"),
    })
}

/// The sender, the reply pane and the text of a prompt the engine built for a peer message.
fn incoming_peer(prompt: &str) -> Option<(&str, &str, &str)> {
    let rest = prompt.strip_prefix("Message from the Soyeht agent ")?;
    let (header, body) = rest.split_once(".\n\n")?;
    let (sender, route) = header.rsplit_once(" MCP server using targetPaneId=")?;
    Some((sender.split(" (").next()?, route, body))
}

fn execute(client: &mut Client) -> Result<Value> {
    for (text, instructions) in [
        ("Plan the change", "Identify the affected surface"),
        ("Implement the change", "Preserve the declared constraints"),
    ] {
        client.call_tool(
            "task_subitem_add",
            json!({"text":text,"instructions":instructions}),
        )?;
    }
    client.call_tool(
        "task_comment",
        json!({"text":"Planning milestone completed"}),
    )?;
    client.call_tool("task_subitem_done", json!({"subitemId":"WS-1-T01"}))?;
    client.call_tool("task_subitem_done", json!({"subitemId":"WS-1-T02"}))?;
    client.call_tool("task_request_review", json!({}))?;
    client.call_tool(
        "task_complete",
        json!({"summary":"Task workflow completed","contractChanges":[],"outOfScopeEdits":[]}),
    )
}

fn string_array(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn environment(value: &Value) -> Option<Vec<(String, String)>> {
    value
        .as_array()?
        .iter()
        .map(|item| {
            Some((
                item["name"].as_str()?.into(),
                item["value"].as_str()?.into(),
            ))
        })
        .collect()
}

fn invalid<T>(message: &str) -> Result<T> {
    Err(io_error(message).into())
}

fn io_error(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
