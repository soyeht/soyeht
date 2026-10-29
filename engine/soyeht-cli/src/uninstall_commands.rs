//! `soyeht uninstall`: stops and unregisters the engine and the supervisor of the profile, and
//! keeps the person's data unless `--delete-data` asks for it (ADR 0118). Like the service
//! commands it never connects to the engine.

use crate::{args::invalid, service_commands::report};
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_protocol::profile::Profile;
use soyeht_service::{Commands, Component, Layout, Manager, Real, remove_data};
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

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
    uninstall(&layout, &Real, &options, &base, ask_on_terminal)
}

/// The steps: confirm first, so a "no" changes nothing; then unregister, engine first; then,
/// when asked, delete the data.
fn uninstall(
    layout: &Layout,
    commands: &impl Commands,
    options: &Options,
    base: &Path,
    ask: impl FnOnce(&Layout) -> Result<String>,
) -> Result<Value> {
    if options.delete_data && !options.yes {
        let answer = ask(layout)?;
        if !confirms(&answer, layout.profile.name) {
            return Err(invalid("Not confirmed; nothing was changed"));
        }
    }
    let manager = Manager::new(layout, commands);
    manager.uninstall(&Component::START_ORDER)?;
    let deleted = options.delete_data && remove_data(layout, base)?;
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
