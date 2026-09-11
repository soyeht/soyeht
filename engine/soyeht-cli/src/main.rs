#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod commands;

use commands::USAGE;
use serde_json::{Value, json};
use soyeht_cli::Client;
use std::{io, path::PathBuf};

fn run() -> io::Result<Value> {
    let mut directory = PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is required"))?,
    )
    .join("Library/Application Support/Soyeht.next/dev");
    let mut args = std::env::args().skip(1).peekable();
    while args.peek().is_some_and(|s| s.starts_with("--")) {
        let argument = args
            .next()
            .ok_or_else(|| io::Error::other("missing command-line argument"))?;
        match argument.as_str() {
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
    commands::execute(&mut client, &args)
}

fn main() {
    match run().and_then(|value| serde_json::to_string_pretty(&value).map_err(io::Error::other)) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{}", json!({"error":error.to_string()}));
            std::process::exit(1);
        }
    }
}
