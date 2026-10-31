//! `soyeht crash-reports`: what the clients ask about the crash reports of the profile (ADR 0127,
//! contract 16). It works with no engine, since the engine is what may have crashed.
//!
//! - `status`: the reports the person has not seen (a count, their names, the newest).
//! - `acknowledge [NAME...]`: the person has seen those reports, or all pending ones when none is
//!   named (they stay for `soyeht diagnostics`). A client names what its notice showed, so a report
//!   that arrived after is still pending.
//! - `import FILE`: turns a crash diagnostic of the system (MetricKit, from the Mac app) into a
//!   report of the program `mac`, and deletes the file. Only a file the app dropped in `crashes/`,
//!   named `incoming-*`, and only a diagnostic.

mod metrickit;

use crate::args::invalid;
use serde_json::{Value, json};
use soyeht_cli::Result;
use soyeht_platform::crash::{self, store};
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

/// The most of a system crash diagnostic that is read; a real one is a few kilobytes.
const IMPORT_MAX_BYTES: u64 = 1024 * 1024;
/// What the name of a file the app drops for `import` starts with.
const INCOMING: &str = "incoming-";

pub fn execute(directory: PathBuf, args: &[String]) -> Result<Value> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["status"] => status(&directory),
        ["acknowledge", names @ ..] => {
            let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
            Ok(json!({"acknowledged": store::acknowledge(&directory, &names)?}))
        }
        ["import", file] => import(&directory, Path::new(file), crash::unix_seconds()),
        _ => Err(invalid(
            "crash-reports status | crash-reports acknowledge [NAME...] | crash-reports import FILE",
        )),
    }
}

fn status(directory: &Path) -> Result<Value> {
    let pending = store::pending(directory)?;
    let latest = pending.last().map(|stored| {
        json!({
            "program": stored.report.program.name(),
            "time": stored.report.time,
        })
    });
    let names: Vec<&str> = pending.iter().map(|stored| stored.name.as_str()).collect();
    Ok(json!({"pending": pending.len(), "reports": names, "latest": latest}))
}

/// `file` is a regular file (not a link) that sits directly in `crashes/` and is named
/// `incoming-*`: the one place the app drops a diagnostic, so `import` cannot be pointed at, and
/// delete, anything else.
fn dropped_file(directory: &Path, file: &Path) -> Result<PathBuf> {
    let folder = soyeht_platform::fs::canonicalize(&store::directory(directory))?;
    let found = soyeht_platform::fs::canonicalize(file)?;
    let named = found
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(INCOMING));
    if found.parent() != Some(folder.as_path()) || !named || !fs::symlink_metadata(file)?.is_file()
    {
        return Err(invalid(
            "A crash diagnostic is imported only from an incoming-* file in crashes/",
        ));
    }
    Ok(found)
}

fn import(directory: &Path, file: &Path, time: u64) -> Result<Value> {
    let file = dropped_file(directory, file)?;
    let mut bytes = Vec::new();
    fs::File::open(&file)?
        .take(IMPORT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > IMPORT_MAX_BYTES {
        return Err(invalid("The crash diagnostic is too large"));
    }
    let diagnostic: Value = serde_json::from_slice(&bytes)?;
    let Some(report) = metrickit::report(&diagnostic, time) else {
        return Err(invalid("The file is not a crash diagnostic"));
    };
    store::write(directory, &report)?;
    fs::remove_file(&file)?;
    Ok(json!({"imported": crash::utc(time)}))
}

#[cfg(test)]
mod tests;
