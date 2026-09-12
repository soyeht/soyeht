use crate::{Error, Result, engine::EnginePort, tools::Service};
use serde_json::{Value, json};
use soyeht_protocol::ndjson::Decoder;
use std::io::{BufRead, Write};

pub fn serve<P: EnginePort>(
    mut input: impl BufRead,
    mut output: impl Write,
    mut service: Service<P>,
) -> Result<()> {
    let mut decoder = Decoder::new(0);
    let mut ended_at_boundary = true;
    let mut initialized = false;
    loop {
        let consumed;
        {
            let available = input.fill_buf()?;
            if available.is_empty() {
                break;
            }
            consumed = available.len().min(8192);
            let bytes = &available[..consumed];
            ended_at_boundary = bytes.last() == Some(&b'\n');
            for record in decoder.push(decoder.next_offset(), bytes)?.lines {
                let request: Value = serde_json::from_slice(&record.bytes)?;
                if let Some(response) = dispatch(&mut service, &mut initialized, request) {
                    serde_json::to_writer(&mut output, &response?)?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                }
            }
        }
        input.consume(consumed);
    }
    if !ended_at_boundary || decoder.buffered_len() != 0 {
        return Err(Error::IncompleteLine);
    }
    Ok(())
}

fn dispatch<P: EnginePort>(
    service: &mut Service<P>,
    initialized: &mut bool,
    request: Value,
) -> Option<Result<Value>> {
    let id = request.get("id")?.clone();
    if request["jsonrpc"] != "2.0" || !valid_id(&id) {
        return Some(Ok(failure(id, -32600, "Invalid request")));
    }
    let method = request["method"].as_str();
    let result = match method {
        Some("initialize") => {
            *initialized = true;
            Ok(json!({
                "protocolVersion":"2025-11-25",
                "capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"Soyeht","version":env!("CARGO_PKG_VERSION")}
            }))
        }
        Some("ping") => Ok(json!({})),
        Some("tools/list" | "tools/call") if !*initialized => {
            return Some(Ok(failure(id, -32002, "Server not initialized")));
        }
        Some("tools/list") => Ok(service.list()),
        Some("tools/call") => {
            let params = &request["params"];
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            params["name"]
                .as_str()
                .ok_or(Error::InvalidArguments("tool name is required"))
                .and_then(|name| service.call(name, arguments))
        }
        _ => return Some(Ok(failure(id, -32601, "Method not found"))),
    };
    Some(Ok(match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(Error::Remote(remote)) => json!({
            "jsonrpc":"2.0","id":id,"result":{
                "content":[{"type":"text","text":remote.message}],
                "structuredContent":{"kind":remote.data.kind,"detail":remote.data.detail},
                "isError":true
            }
        }),
        Err(
            Error::UnknownTool
            | Error::ToolForbidden
            | Error::TaskContextRequired
            | Error::InvalidArguments(_),
        ) => failure(id, -32602, "Invalid tool request"),
        Err(_) => failure(id, -32603, "Task service unavailable"),
    }))
}

fn valid_id(id: &Value) -> bool {
    id.is_string() || id.is_i64() || id.is_u64()
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::TaskContext;
    use soyeht_protocol::Role;
    use std::io::Cursor;

    struct FakeEngine {
        role: Role,
    }
    impl EnginePort for FakeEngine {
        fn role(&self) -> &Role {
            &self.role
        }
        fn request(&mut self, method: &str, params: Value) -> Result<Value> {
            Ok(json!({"method":method,"paneId":params["paneId"]}))
        }
    }

    #[test]
    fn stdio_server_negotiates_lists_tools_and_returns_structured_results() {
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"task_next\",\"arguments\":{}}}\n"
        );
        let context = TaskContext {
            workspace_id: "w".into(),
            pane_id: "p".into(),
            session_generation: 4,
            task_id: None,
            attempt: None,
        };
        let mut output = Vec::new();
        serve(
            Cursor::new(input),
            &mut output,
            Service::new(FakeEngine { role: Role::Agent }, context),
        )
        .unwrap();
        let replies = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-11-25");
        assert!(
            replies[1]["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "task_next")
        );
        assert_eq!(
            replies[2]["result"]["structuredContent"]["method"],
            "task.next"
        );
        assert_eq!(replies[2]["result"]["structuredContent"]["paneId"], "p");
    }

    #[test]
    fn oversized_input_is_discarded_without_hiding_the_next_exact_request() {
        let mut input = vec![b'x'; soyeht_protocol::limits::ACP_MESSAGE_BYTES + 1];
        input.extend_from_slice(
            b"\n{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ping\",\"params\":{}}\n",
        );
        let context = TaskContext {
            workspace_id: "w".into(),
            pane_id: "p".into(),
            session_generation: 4,
            task_id: None,
            attempt: None,
        };
        let mut output = Vec::new();
        serve(
            Cursor::new(input),
            &mut output,
            Service::new(FakeEngine { role: Role::Agent }, context),
        )
        .unwrap();
        let reply: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(reply["id"], 4);
        assert_eq!(reply["result"], json!({}));
    }

    #[test]
    fn malformed_and_early_tool_requests_never_reach_the_engine() {
        let input = concat!(
            "{\"jsonrpc\":\"1.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"task_next\",\"arguments\":{}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"task_next\",\"arguments\":{}}}\n"
        );
        let context = TaskContext {
            workspace_id: "w".into(),
            pane_id: "p".into(),
            session_generation: 4,
            task_id: None,
            attempt: None,
        };
        let mut output = Vec::new();
        serve(
            Cursor::new(input),
            &mut output,
            Service::new(FakeEngine { role: Role::Agent }, context),
        )
        .unwrap();
        let replies = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(replies[0]["error"]["code"], -32600);
        assert_eq!(replies[1]["error"]["code"], -32002);
        assert_eq!(
            replies[3]["result"]["structuredContent"]["method"],
            "task.next"
        );
    }
}
