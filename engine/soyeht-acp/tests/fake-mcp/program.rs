//! Deterministic task-tool server used only by the ACP integration tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{self, BufRead, Write},
    path::PathBuf,
};

const TOOLS: [&str; 8] = [
    "task_next",
    "task_subitem_add",
    "task_subitem_done",
    "task_comment",
    "task_block",
    "task_request_review",
    "task_complete",
    "task_release",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--log") {
        return Err("--log is required".into());
    }
    let log = PathBuf::from(args.next().ok_or("log path is required")?);
    if !log.is_absolute() || args.next().is_some() {
        return Err("log path must be one absolute argument".into());
    }
    let mut output = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let result = match request["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion":"2025-11-25",
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"Soyeht Task Test Server","version":"1"}
            }),
            Some("tools/list") => json!({"tools":TOOLS.map(tool)}),
            Some("tools/call") => call(&log, &request["params"]),
            _ => {
                write_value(
                    &mut output,
                    &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
                )?;
                continue;
            }
        };
        write_value(
            &mut output,
            &json!({"jsonrpc":"2.0","id":id,"result":result}),
        )?;
    }
    Ok(())
}

fn tool(name: &str) -> Value {
    json!({
        "name":name,
        "description":format!("Deterministic {name} test tool"),
        "inputSchema":{"type":"object","additionalProperties":true}
    })
}

fn call(log: &PathBuf, params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or("missing");
    let record = json!({"name":name,"arguments":params["arguments"]});
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .expect("open tool log");
    writeln!(file, "{}", serde_json::to_string(&record).unwrap()).expect("write tool log");
    file.sync_data().expect("sync tool log");
    let content = if name == "task_next" {
        json!({"task":{"taskId":"WS-1"},"willRestartSession":true})
    } else {
        json!({"accepted":true,"tool":name})
    };
    json!({
        "content":[{"type":"text","text":serde_json::to_string(&content).unwrap()}],
        "structuredContent":content,
        "isError":false
    })
}

fn write_value(output: &mut impl Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    output.flush()
}
