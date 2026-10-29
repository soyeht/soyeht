//! `soyeht diagnostics`: the report a person pastes into an issue (ADR 0118). It works with no
//! engine, which is when it is most needed: it asks the engine for the readiness checklist only
//! when one answers, and reads the log files either way.

use crate::args::Flags;
use soyeht_cli::{Client, Result};
use soyeht_platform::{
    diagnostics::{
        Check, DEFAULT_LOG_LINES, Input, MAX_LOG_LINES, Presence, Readiness, Redactor, report,
    },
    local::connect,
};
use soyeht_protocol::{
    EngineReadinessResult, method,
    profile::{DESKTOP_LOG, ENGINE_LOG, ENGINE_SOCKET, PTYD_LOG, PTYD_SOCKET, Profile},
};
use std::path::{Path, PathBuf};

pub fn execute(profile: &'static Profile, directory: PathBuf, args: &[String]) -> Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let flags = Flags::parse(&args, &["--lines"])?;
    let lines = flags
        .number("--lines")?
        .map_or(DEFAULT_LOG_LINES, |lines| lines as usize)
        .clamp(1, MAX_LOG_LINES);
    Ok(gather(
        profile,
        &directory,
        lines,
        &Redactor::current(),
        ask_readiness,
    ))
}

/// The report for the profile whose data is in `directory`. `readiness` asks the engine, and is
/// called only when the engine answers on its endpoint.
fn gather(
    profile: &Profile,
    directory: &Path,
    lines: usize,
    redactor: &Redactor,
    readiness: impl FnOnce(&Path) -> Readiness,
) -> String {
    let presence = |socket: &str| {
        if connect(&directory.join(socket)).is_ok() {
            Presence::Running
        } else {
            Presence::NotRunning
        }
    };
    let engine = presence(ENGINE_SOCKET);
    report(&Input {
        product_version: env!("CARGO_PKG_VERSION"),
        profile: profile.name,
        engine,
        supervisor: presence(PTYD_SOCKET),
        readiness: match engine {
            Presence::Running => readiness(directory),
            Presence::NotRunning => Readiness::Unavailable,
        },
        data: directory,
        logs: &[ENGINE_LOG, PTYD_LOG, DESKTOP_LOG],
        lines,
        redactor,
    })
}

/// The checklist the running engine reports. An engine that does not answer is not an error
/// here: the report says so.
fn ask_readiness(directory: &Path) -> Readiness {
    Client::connect(directory)
        .and_then(|mut client| {
            client.request::<EngineReadinessResult>(method::ENGINE_READINESS, serde_json::json!({}))
        })
        .map_or(Readiness::Unavailable, |result| checked(&result))
}

fn checked(result: &EngineReadinessResult) -> Readiness {
    Readiness::Checked {
        system: serde_json::to_value(&result.system)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
        checks: result
            .items
            .iter()
            .map(|item| Check {
                id: item.id.clone(),
                required: item.required,
                found: item.found,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests;
