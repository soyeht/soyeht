#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod commands;
mod editor_commands;
mod task_commands;

use commands::USAGE;
use serde_json::{Value, json};
use soyeht_cli::{Client, Error, Result};
use soyeht_protocol::data_directory::dev_data_directory;
use std::path::PathBuf;

fn run() -> Result<Value> {
    let mut directory = dev_data_directory(std::env::home_dir())?;
    let mut args = std::env::args().skip(1).peekable();
    let mut bound = false;
    while args.peek().is_some_and(|s| s.starts_with("--")) {
        let argument = args.next().ok_or(Error::MissingArgument)?;
        match argument.as_str() {
            "--profile" => {
                if args.next().as_deref() != Some("dev") {
                    return Err(Error::UnsupportedProfile);
                }
            }
            "--data-dir" => {
                directory = PathBuf::from(args.next().ok_or(Error::MissingDataDirectory)?)
            }
            "--help" => return Ok(json!({"usage":USAGE})),
            "--bound" => bound = true,
            _ => return Err(Error::InvalidArguments(USAGE.into())),
        }
    }
    let args: Vec<_> = args.collect();
    let hook = args.first().is_some_and(|arg| arg == "__experiment-hook");
    let connected = if hook {
        if bound {
            return Err(Error::InvalidArguments(
                "Experiment hook cannot use an agent binding".into(),
            ));
        }
        Client::connect_hook(&directory)
    } else if bound {
        let binding = std::env::var("SOYEHT_MCP_BINDING").map_err(|_| Error::MissingBinding)?;
        Client::connect_bound(&directory, &binding)
    } else {
        Client::connect(&directory)
    };
    let mut client = connected.map_err(|error| Error::EngineUnavailable(Box::new(error)))?;
    commands::execute(&mut client, &args)
}

fn main() {
    match run().and_then(|value| serde_json::to_string_pretty(&value).map_err(Error::from)) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{}", json!({"error":error.to_string()}));
            std::process::exit(1);
        }
    }
}
