//! `soyeht service`: registering the engine and its supervisor with the system's service manager
//! (ADR 0114). These commands never connect to the engine, which may not be running.

use crate::args::{Flags, invalid};
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_protocol::profile::Profile;
use soyeht_service::{Commands, Component, Definition, Layout, Manager, Real, System, render};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn execute(profile: &'static Profile, directory: PathBuf, args: &[String]) -> Result<Value> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["render", rest @ ..] => render_text(rest),
        ["install", rest @ ..] => {
            let flags = Flags::parse(rest, &["--component", "--programs"])?;
            let programs = match flags.get("--programs") {
                Some(programs) => PathBuf::from(programs),
                None => own_folder()?,
            };
            let components = components(&flags)?;
            let layout = Layout::current(profile, directory)?;
            let manager = Manager::new(&layout, &Real);
            manager.install(&programs, &components)?;
            report(&manager, &components)
        }
        ["status", rest @ ..] => {
            let flags = Flags::parse(rest, &["--component", "--programs"])?;
            let components = components(&flags)?;
            let layout = Layout::current(profile, directory)?;
            let manager = Manager::new(&layout, &Real);
            let mut result = report(&manager, &components)?;
            if let Some(programs) = flags.get("--programs") {
                add_current(&manager, &mut result, Path::new(programs))?;
            }
            Ok(result)
        }
        [action @ ("uninstall" | "start" | "stop"), rest @ ..] => {
            let flags = Flags::parse(rest, &["--component"])?;
            let components = components(&flags)?;
            let layout = Layout::current(profile, directory)?;
            let manager = Manager::new(&layout, &Real);
            match *action {
                "uninstall" => manager.uninstall(&components)?,
                "start" => manager.start(&components)?,
                _ => manager.stop(&components)?,
            }
            report(&manager, &components)
        }
        _ => Err(invalid(
            "Expected service install, uninstall, start, stop, status or render",
        )),
    }
}

/// The folder of this program, where a release keeps the engine and the supervisor beside it.
fn own_folder() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid("The folder of this program is unknown; pass --programs"))
}

/// The component the flag names, or both.
fn components(flags: &Flags) -> Result<Vec<Component>> {
    match flags.get("--component") {
        None => Ok(Component::START_ORDER.to_vec()),
        Some(name) => Component::parse(name)
            .map(|component| vec![component])
            .ok_or_else(|| invalid("--component must be engine or ptyd")),
    }
}

/// What the system says about each component after the command.
pub(crate) fn report(
    manager: &Manager<'_, impl Commands>,
    components: &[Component],
) -> Result<Value> {
    let mut services = Vec::new();
    for component in Component::START_ORDER {
        if components.contains(&component) {
            let status = manager.status(component)?;
            services.push(json!({
                "component": status.component.name(),
                "name": status.name,
                "registered": status.registered,
                "running": status.running,
            }));
        }
    }
    Ok(json!({ "services": services }))
}

/// Adds `"current"` to each service of a status report: whether its definition is exactly what
/// `install --programs PROGRAMS` would write (contract 13, section 3).
fn add_current(
    manager: &Manager<'_, impl Commands>,
    result: &mut Value,
    programs: &Path,
) -> Result<()> {
    if let Some(services) = result["services"].as_array_mut() {
        for service in services {
            let component = service["component"]
                .as_str()
                .and_then(Component::parse)
                .ok_or_else(|| invalid("The report names a component that does not exist"))?;
            service["current"] = json!(manager.is_current(programs, component)?);
        }
    }
    Ok(())
}

/// `service render`: the text of a service file for values given on the command line, without
/// touching the system. The scripts that start disposable services for tests use it, so that no
/// script keeps a second copy of what a service file says.
fn render_text(args: &[&str]) -> Result<Value> {
    let (options, command) = match args.iter().position(|arg| *arg == "--") {
        Some(split) => (&args[..split], &args[split + 1..]),
        None => return Err(invalid("service render needs -- and then the program")),
    };
    let (program, arguments) = command
        .split_first()
        .ok_or_else(|| invalid("service render needs a program after --"))?;
    let mut system = None;
    let mut definition = Definition {
        name: String::new(),
        description: String::new(),
        program: (*program).to_owned(),
        arguments: arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
        environment: BTreeMap::new(),
        log: None,
        files: Vec::new(),
        after: None,
        user: None,
    };
    let mut options = options.iter();
    while let Some(flag) = options.next() {
        let value = options
            .next()
            .ok_or_else(|| invalid(format!("Missing value for {flag}")))?;
        match *flag {
            "--system" => {
                system = Some(System::parse(value).ok_or_else(|| invalid("Unknown --system"))?);
            }
            "--name" => definition.name = (*value).to_owned(),
            "--description" => definition.description = (*value).to_owned(),
            "--log" => definition.log = Some((*value).to_owned()),
            "--after" => definition.after = Some((*value).to_owned()),
            "--user" => definition.user = Some((*value).to_owned()),
            "--env" => {
                let (name, text) = value
                    .split_once('=')
                    .ok_or_else(|| invalid("--env takes KEY=VALUE"))?;
                definition
                    .environment
                    .insert(name.to_owned(), text.to_owned());
            }
            other => return Err(invalid(format!("Unknown option: {other}"))),
        }
    }
    let system = system.ok_or_else(|| invalid("Missing required option: --system"))?;
    Ok(json!({ "text": render(system, &definition)? }))
}
