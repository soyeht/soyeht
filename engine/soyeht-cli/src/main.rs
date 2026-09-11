use serde_json::{Value, json};
use soyeht_cli::Client;
use soyeht_protocol::{
    DeviceListResult, DevicePairStartResult, EngineStatusResult, PaneCloseResult, PaneCreateResult,
    WorkspaceCreateResult, WorkspaceListResult,
};
use std::{io, path::PathBuf};
use uuid::Uuid;

const USAGE: &str = "soyeht --profile dev [--data-dir PATH] status | pane create --shell [--cwd PATH] [--idempotency-key KEY] | pane list | pane close ID | device pair-start | device list";

fn run() -> io::Result<Value> {
    let mut directory = PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is required"))?,
    )
    .join("Library/Application Support/Soyeht.next/dev");
    let mut args = std::env::args().skip(1).peekable();
    while args.peek().is_some_and(|s| s.starts_with("--")) {
        match args.next().expect("argument").as_str() {
            "--profile" => {
                if args.next().as_deref() != Some("dev") {
                    return Err(io::Error::other("only the new dev profile is available"));
                }
            }
            "--data-dir" => {
                directory = PathBuf::from(
                    args.next()
                        .ok_or_else(|| io::Error::other("missing data directory"))?,
                )
            }
            "--help" => return Ok(json!({"usage":USAGE})),
            _ => return Err(io::Error::other(USAGE)),
        }
    }
    let args: Vec<_> = args.collect();
    let mut client = Client::connect(&directory).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot connect to the new Dev engine: {error}"),
        )
    })?;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["status"] => Ok(serde_json::to_value(
            client.request::<EngineStatusResult>("engine.status", json!({}))?,
        )?),
        ["pane", "list"] => {
            Ok(json!({"items":client.snapshot()?.sections.remove("panes").unwrap_or_default()}))
        }
        ["pane", "close", id] => Ok(serde_json::to_value(client.request::<PaneCloseResult>(
            "pane.close",
            json!({"paneId":id,"idempotencyKey":Uuid::new_v4().to_string()}),
        )?)?),
        ["device", "list"] => Ok(serde_json::to_value(
            client.request::<DeviceListResult>("device.list", json!({}))?,
        )?),
        ["device", "pair-start"] => Ok(serde_json::to_value(
            client.request::<DevicePairStartResult>(
                "device.pair.start",
                json!({"idempotencyKey":Uuid::new_v4().to_string()}),
            )?,
        )?),
        ["pane", "create", "--shell", rest @ ..] => {
            let mut cwd = std::env::current_dir()?;
            let mut key = Uuid::new_v4().to_string();
            let mut options = rest.iter();
            while let Some(option) = options.next() {
                match *option {
                    "--cwd" => {
                        cwd = PathBuf::from(
                            options
                                .next()
                                .ok_or_else(|| io::Error::other("missing working directory"))?,
                        )
                    }
                    "--idempotency-key" => {
                        key = options
                            .next()
                            .ok_or_else(|| io::Error::other("missing idempotency key"))?
                            .to_string()
                    }
                    _ => return Err(io::Error::other(USAGE)),
                }
            }
            let cwd = cwd.canonicalize()?;
            let path = cwd
                .to_str()
                .ok_or_else(|| io::Error::other("directory must be valid UTF-8"))?;
            let workspaces: WorkspaceListResult = client.request("workspace.list", json!({}))?;
            let workspace = if let Some(workspace) =
                workspaces.items.into_iter().find(|w| w.path == path)
            {
                workspace
            } else {
                let created:WorkspaceCreateResult=client.request("workspace.create",json!({"name":cwd.file_name().and_then(|s|s.to_str()).unwrap_or("Workspace"),"path":path,"idempotencyKey":Uuid::new_v4().to_string()}))?;
                created.workspace
            };
            Ok(serde_json::to_value(client.request::<PaneCreateResult>("pane.create",json!({"workspaceId":workspace.workspace_id,"mode":"terminal","cwd":path,"idempotencyKey":key}))?)?)
        }
        _ => Err(io::Error::other(USAGE)),
    }
}

fn main() {
    match run() {
        Ok(value) => println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("JSON output")
        ),
        Err(error) => {
            eprintln!("{}", json!({"error":error.to_string()}));
            std::process::exit(1);
        }
    }
}
