#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_mcp::{
    Error, Result,
    engine::Client,
    server,
    tools::{Service, TaskContext},
};
use soyeht_protocol::{
    data_directory::resolve,
    handshake::PANE_BINDING_ENV,
    profile::{ENGINE_SOCKET, SELECTION_ENV},
};
use std::{io, path::PathBuf};

const BINDING_ENV: &str = "SOYEHT_MCP_BINDING";

struct Config {
    data_dir: PathBuf,
    context: TaskContext,
}

/// What the server speaks for: an agent pane the engine named in the options, or a terminal
/// pane whose binding is in the environment (ADR 0165), which needs no option at all.
enum Mode {
    Pane(Config),
    Terminal { data_dir: Option<PathBuf> },
}

fn arguments() -> Result<Mode> {
    arguments_from(std::env::args().skip(1))
}

fn arguments_from(args: impl IntoIterator<Item = String>) -> Result<Mode> {
    let mut data_dir = None;
    let mut workspace_id = None;
    let mut pane_id = None;
    let mut generation = None;
    let mut task_id = None;
    let mut attempt = None;
    let mut args = args.into_iter();
    while let Some(name) = args.next() {
        let value = args
            .next()
            .ok_or(Error::InvalidArguments("every option requires a value"))?;
        match name.as_str() {
            "--data-dir" => set_once(&mut data_dir, PathBuf::from(value))?,
            "--workspace-id" => set_once(&mut workspace_id, value)?,
            "--pane-id" => set_once(&mut pane_id, value)?,
            "--session-generation" => {
                let value = value.parse().map_err(|_| {
                    Error::InvalidArguments("session generation must be an integer")
                })?;
                set_once(&mut generation, value)?;
            }
            "--task-id" => set_once(&mut task_id, value)?,
            "--attempt" => {
                let value = value
                    .parse()
                    .map_err(|_| Error::InvalidArguments("attempt must be an integer"))?;
                set_once(&mut attempt, value)?;
            }
            _ => return Err(Error::InvalidArguments("unknown option")),
        }
    }
    let pane_options = workspace_id.is_some()
        || pane_id.is_some()
        || generation.is_some()
        || task_id.is_some()
        || attempt.is_some();
    if !pane_options {
        if data_dir
            .as_ref()
            .is_some_and(|directory| !directory.is_absolute())
        {
            return Err(Error::InvalidArguments("data directory must be absolute"));
        }
        return Ok(Mode::Terminal { data_dir });
    }
    if task_id.is_some() != attempt.is_some() {
        return Err(Error::InvalidArguments(
            "task and attempt must be supplied together",
        ));
    }
    let data_dir = data_dir.ok_or(Error::InvalidArguments("data directory is required"))?;
    if !data_dir.is_absolute() {
        return Err(Error::InvalidArguments("data directory must be absolute"));
    }
    Ok(Mode::Pane(Config {
        data_dir,
        context: TaskContext {
            workspace_id: required(workspace_id, "workspace identity is required")?,
            pane_id: required(pane_id, "pane identity is required")?,
            session_generation: generation
                .ok_or(Error::InvalidArguments("session generation is required"))?,
            task_id,
            attempt,
        },
    }))
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(Error::InvalidArguments("duplicate option"));
    }
    Ok(())
}

fn required(value: Option<String>, message: &'static str) -> Result<String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or(Error::InvalidArguments(message))
}

fn run() -> Result<()> {
    match arguments()? {
        Mode::Pane(config) => run_pane(config),
        Mode::Terminal { data_dir } => run_terminal(data_dir),
    }
}

fn run_pane(config: Config) -> Result<()> {
    // ignored: an unset or non-Unicode variable is reported by `required` as a missing binding
    let binding = required(std::env::var(BINDING_ENV).ok(), "binding is required")?;
    let client = Client::connect(&config.data_dir.join(ENGINE_SOCKET), &binding)?;
    server::serve(
        io::stdin().lock(),
        io::stdout().lock(),
        Service::new(client, config.context),
        &server::instructions(&soyeht_skills::home::user_store(&config.data_dir)),
    )
}

/// The server of a terminal pane, with one connection to the engine for as long as the harness
/// keeps it. Started where the environment has no pane binding (the person runs the harness
/// anywhere), it answers the handshake with no tools and says why, and does not fail.
fn run_terminal(data_dir: Option<PathBuf>) -> Result<()> {
    let binding = match std::env::var(PANE_BINDING_ENV) {
        Ok(binding) if !binding.is_empty() => binding,
        _ => {
            return server::serve(
                io::stdin().lock(),
                io::stdout().lock(),
                Service::outside(),
                &server::outside_instructions(),
            );
        }
    };
    let data_dir = match data_dir {
        Some(directory) => directory,
        None => {
            resolve(
                std::env::var_os(SELECTION_ENV).as_deref(),
                None,
                soyeht_platform::dirs::data_base(),
            )?
            .1
        }
    };
    let client = Client::connect(&data_dir.join(ENGINE_SOCKET), &binding)?;
    server::serve(
        io::stdin().lock(),
        io::stdout().lock(),
        Service::terminal(client),
        &server::terminal_instructions(&soyeht_skills::home::user_store(&data_dir)),
    )
}

fn main() {
    if let Err(error) = run() {
        eprintln!("soyeht-mcp: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<Mode> {
        arguments_from(values.iter().map(|value| (*value).to_owned()))
    }

    fn pane(mode: Result<Mode>) -> Config {
        match mode.unwrap() {
            Mode::Pane(config) => config,
            Mode::Terminal { .. } => panic!("not an agent pane"),
        }
    }

    #[test]
    fn parser_requires_complete_unique_pane_identity() {
        let data = std::env::temp_dir().join("data");
        let data = data.to_str().unwrap();
        let valid = [
            "--data-dir",
            data,
            "--workspace-id",
            "workspace",
            "--pane-id",
            "pane",
            "--session-generation",
            "4",
            "--task-id",
            "WS-1",
            "--attempt",
            "2",
        ];
        let config = pane(parse(&valid));
        assert_eq!(config.context.task_id.as_deref(), Some("WS-1"));
        assert_eq!(config.context.attempt, Some(2));

        let mut duplicate = valid.to_vec();
        duplicate.extend(["--pane-id", "other"]);
        assert!(matches!(
            parse(&duplicate),
            Err(Error::InvalidArguments("duplicate option"))
        ));
        assert!(matches!(
            parse(&valid[..valid.len() - 2]),
            Err(Error::InvalidArguments(
                "task and attempt must be supplied together"
            ))
        ));
    }

    #[test]
    fn no_pane_option_is_a_terminal_pane_and_a_part_of_one_is_refused() {
        assert!(matches!(parse(&[]), Ok(Mode::Terminal { data_dir: None })));
        let data = std::env::temp_dir().join("data");
        let data_text = data.to_str().unwrap();
        assert!(matches!(
            parse(&["--data-dir", data_text]),
            Ok(Mode::Terminal { data_dir: Some(directory) }) if directory == data
        ));
        assert!(matches!(
            parse(&["--data-dir", "relative"]),
            Err(Error::InvalidArguments("data directory must be absolute"))
        ));
        // One option of an agent pane makes it one, and then every one is required.
        assert!(matches!(
            parse(&["--data-dir", data_text, "--pane-id", "pane"]),
            Err(Error::InvalidArguments("workspace identity is required"))
        ));
        assert!(matches!(
            parse(&["--pane-id", "pane"]),
            Err(Error::InvalidArguments("data directory is required"))
        ));
    }
}
