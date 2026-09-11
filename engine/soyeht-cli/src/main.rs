#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod commands;

use commands::USAGE;
use serde_json::{Value, json};
use soyeht_cli::{Client, Error, Result};
use std::path::PathBuf;

fn run() -> Result<Value> {
    let mut directory = PathBuf::from(std::env::var_os("HOME").ok_or(Error::HomeMissing)?)
        .join("Library/Application Support/Soyeht.next/dev");
    let mut args = std::env::args().skip(1).peekable();
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
            _ => return Err(Error::InvalidArguments(USAGE.into())),
        }
    }
    let args: Vec<_> = args.collect();
    let mut client =
        Client::connect(&directory).map_err(|error| Error::EngineUnavailable(Box::new(error)))?;
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
