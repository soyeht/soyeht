//! `soyeht uninstall`: stops and unregisters the engine and the supervisor of the profile, and
//! keeps the person's data unless `--delete-data` asks for it (ADR 0118). Like the service
//! commands it never connects to the engine.

use crate::{args::invalid, service_commands::report};
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_protocol::profile::Profile;
use soyeht_service::{Commands, Component, Layout, Manager, Real, plan_removal};
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// How long the programs are given to end, after their services are unregistered, before the
/// data they hold open is deleted.
const STOP_WAIT: Duration = Duration::from_secs(15);

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    delete_data: bool,
    yes: bool,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options::default();
    for arg in args {
        match *arg {
            "--delete-data" => options.delete_data = true,
            "--yes" => options.yes = true,
            other => return Err(invalid(format!("Unknown option: {other}"))),
        }
    }
    if options.yes && !options.delete_data {
        return Err(invalid("--yes confirms --delete-data; pass both"));
    }
    Ok(options)
}

pub fn execute(profile: &'static Profile, directory: PathBuf, args: &[String]) -> Result<Value> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let options = parse(&args)?;
    let layout = Layout::current(profile, directory)?;
    let base = soyeht_platform::dirs::data_base()
        .ok_or(soyeht_service::Error::Environment("the data folder"))?;
    uninstall(
        &layout,
        &Real,
        &options,
        (&base, STOP_WAIT),
        ask_on_terminal,
    )
}

/// The steps: check the deletion, which changes nothing; confirm, so a "no" changes nothing
/// either; then unregister, engine first; then delete. A deletion that would be refused is
/// refused before any service is touched. `place` is the system's folder for application data
/// and how long the programs get to stop.
fn uninstall(
    layout: &Layout,
    commands: &impl Commands,
    options: &Options,
    place: (&Path, Duration),
    ask: impl FnOnce(&Layout) -> Result<String>,
) -> Result<Value> {
    let (base, wait) = place;
    let removal = if options.delete_data {
        plan_removal(layout, base)?
    } else {
        None
    };
    if options.delete_data && !options.yes {
        let answer = ask(layout)?;
        if !confirms(&answer, layout.profile.name) {
            return Err(invalid("Not confirmed; nothing was changed"));
        }
    }
    let manager = Manager::new(layout, commands);
    manager.uninstall(&Component::START_ORDER)?;
    let deleted = match removal {
        Some(removal) => {
            removal.execute(wait)?;
            true
        }
        None => false,
    };
    let mut result = report(&manager, &Component::START_ORDER)?;
    result["dataDeleted"] = json!(deleted);
    Ok(result)
}

/// Whether `answer` is the name of the profile whose data is about to go.
fn confirms(answer: &str, profile: &str) -> bool {
    answer.trim() == profile
}

/// The question, on standard error, and the line the person types. Without a terminal there is
/// nobody to ask, and a script says `--yes`.
fn ask_on_terminal(layout: &Layout) -> Result<String> {
    if !io::stdin().is_terminal() {
        return Err(invalid(
            "--delete-data asks for confirmation on a terminal; pass --yes to confirm without one",
        ));
    }
    let profile = layout.profile.name;
    write!(
        io::stderr(),
        "This deletes ALL Soyeht data of the {profile} profile (workspaces, transcripts, \
         settings, logs) in {}.\nIt cannot be undone. Type the profile name ({profile}) to \
         confirm: ",
        layout.data.display()
    )?;
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer)
}

#[cfg(test)]
mod tests;
