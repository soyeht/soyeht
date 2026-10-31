//! `soyeht crash-reports`: what the clients ask about the crash reports of the profile (ADR 0127,
//! contract 16). It works with no engine, since the engine is what may have crashed.
//!
//! - `status`: how many reports the person has not seen, and the newest of them.
//! - `acknowledge`: the person has seen them (they stay for `soyeht diagnostics`).
//! - `import FILE`: turns a crash diagnostic of the system (MetricKit, from the Mac app) into a
//!   report of the program `mac`, and deletes the file.

mod metrickit;

use crate::args::invalid;
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_platform::crash::{self, Report, store};
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The most of a system crash diagnostic that is read; a real one is a few kilobytes.
const IMPORT_MAX_BYTES: u64 = 1024 * 1024;

pub fn execute(directory: PathBuf, args: &[String]) -> Result<Value> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["status"] => status(&directory),
        ["acknowledge"] => Ok(json!({"acknowledged": store::acknowledge(&directory)?})),
        ["import", file] => import(&directory, Path::new(file), now()),
        _ => Err(invalid(
            "crash-reports status | crash-reports acknowledge | crash-reports import FILE",
        )),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn status(directory: &Path) -> Result<Value> {
    let pending = store::pending(directory)?;
    let latest = pending.last().map(|stored| {
        json!({
            "program": stored.report.program.name(),
            "time": stored.report.time,
        })
    });
    Ok(json!({"pending": pending.len(), "latest": latest}))
}

fn import(directory: &Path, file: &Path, time: u64) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(file)?
        .take(IMPORT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > IMPORT_MAX_BYTES {
        return Err(invalid("The crash diagnostic is too large"));
    }
    let diagnostic: Value = serde_json::from_slice(&bytes)?;
    let report: Report = metrickit::report(&diagnostic, time);
    store::write(directory, &report)?;
    fs::remove_file(file)?;
    Ok(json!({"imported": crash::utc(time)}))
}

#[cfg(test)]
mod tests;
