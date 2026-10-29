#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod args;
mod commands;
mod diagnostics_commands;
mod editor_commands;
mod service_commands;
mod task_commands;
mod uninstall_commands;

use commands::USAGE;
use serde_json::{Value, json};
use soyeht_cli::{Client, Error, Result};
use soyeht_protocol::{data_directory::resolve, profile::SELECTION_ENV};
use std::path::PathBuf;

/// What a command prints: the JSON every command answers with, or a text meant to be read or
/// pasted as it is.
enum Output {
    Json(Value),
    Text(String),
}

fn run() -> Result<Output> {
    let mut requested = std::env::var_os(SELECTION_ENV);
    let mut directory = None;
    let mut args = std::env::args().skip(1).peekable();
    let mut bound = false;
    while args.peek().is_some_and(|s| s.starts_with("--")) {
        let argument = args.next().ok_or(Error::MissingArgument)?;
        match argument.as_str() {
            "--profile" => requested = Some(args.next().ok_or(Error::MissingArgument)?.into()),
            "--data-dir" => {
                directory = Some(PathBuf::from(
                    args.next().ok_or(Error::MissingDataDirectory)?,
                ))
            }
            "--help" => return Ok(Output::Json(json!({"usage":USAGE}))),
            "--bound" => bound = true,
            _ => return Err(Error::InvalidArguments(USAGE.into())),
        }
    }
    let args: Vec<_> = args.collect();
    let (profile, directory) = resolve(
        requested.as_deref(),
        directory,
        soyeht_platform::dirs::data_base(),
    )?;
    // These commands talk to the system's service manager and the files of the profile, not to
    // the engine, which may not be running (yet, or any more).
    match args.first().map(String::as_str) {
        Some("service") => {
            return service_commands::execute(profile, directory, &args[1..]).map(Output::Json);
        }
        Some("uninstall") => {
            return uninstall_commands::execute(profile, directory, &args[1..]).map(Output::Json);
        }
        Some("diagnostics") => {
            return diagnostics_commands::execute(profile, directory, &args[1..]).map(Output::Text);
        }
        _ => {}
    }
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
    commands::execute(&mut client, &args).map(Output::Json)
}

fn main() {
    let printed = run().and_then(|output| match output {
        Output::Json(value) => serde_json::to_string_pretty(&value).map_err(Error::from),
        Output::Text(text) => Ok(text.trim_end().to_owned()),
    });
    match printed {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{}", json!({"error":error.to_string()}));
            std::process::exit(1);
        }
    }
}
