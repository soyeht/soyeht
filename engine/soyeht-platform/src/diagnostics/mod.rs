//! The diagnostics report: what a person pastes into an issue when something breaks.
//!
//! One function builds it ([`report`]) from facts the caller gathers (the version, the profile,
//! whether the programs run, the readiness items) and from the profile's own log files. Every
//! line that comes from the machine passes through a [`Redactor`] first: no path, no user name,
//! no token and no address leaves it. The logs hold failure kinds and event names, never terminal
//! contents (ADRs 0082, 0088); the redaction is the second guard, for a line that carries more
//! than it should.

mod redact;

pub use redact::Redactor;

use crate::{fs::OpenOptionsPrivateExt as _, process::detached::logs_directory};
use std::{
    fs::OpenOptions,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

/// How many lines of each log the report takes unless the caller asks for another number.
pub const DEFAULT_LOG_LINES: usize = 40;
/// The most lines of each log a report takes.
pub const MAX_LOG_LINES: usize = 400;
/// How much of the end of a log file is read to find its last lines.
const TAIL_BYTES: u64 = 256 * 1024;
/// A log line is cut here, so one long record cannot swamp an issue.
const LINE_CHARS: usize = 600;

/// Whether a program of the engine answers on its endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Running,
    NotRunning,
}

/// One row of the engine's readiness checklist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub id: String,
    pub required: bool,
    pub found: bool,
}

/// What the engine said about the machine, or that it could not be asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// The engine was not asked, or did not answer.
    Unavailable,
    /// `system` is the family the engine named (`macos`, `debian`, ...).
    Checked { system: String, checks: Vec<Check> },
}

/// The facts the caller gathers.
pub struct Input<'a> {
    pub product_version: &'a str,
    pub profile: &'a str,
    pub engine: Presence,
    pub supervisor: Presence,
    pub readiness: Readiness,
    /// The profile's data directory, where `logs/` is.
    pub data: &'a Path,
    /// The log files to include, by file name.
    pub logs: &'a [&'a str],
    /// How many lines of each (at most [`MAX_LOG_LINES`]).
    pub lines: usize,
    pub redactor: &'a Redactor,
}

/// The report as Markdown, ready to paste into an issue.
pub fn report(input: &Input<'_>) -> String {
    let presence = |presence: Presence| match presence {
        Presence::Running => "running",
        Presence::NotRunning => "not running",
    };
    let mut lines = vec![
        "## Soyeht diagnostics".to_owned(),
        String::new(),
        format!("- version: {}", input.product_version),
        format!("- profile: {}", input.profile),
        format!(
            "- system: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!("- engine: {}", presence(input.engine)),
        format!("- supervisor: {}", presence(input.supervisor)),
        String::new(),
        "### Readiness".to_owned(),
        String::new(),
    ];
    match &input.readiness {
        Readiness::Unavailable => {
            lines.push("The engine did not answer, so nothing was checked.".to_owned());
        }
        Readiness::Checked { system, checks } => {
            lines.push(format!("- family: {}", input.redactor.redact(system)));
            for check in checks {
                let id = input.redactor.redact(&check.id);
                let state = match (check.found, check.required) {
                    (true, _) => "found",
                    (false, true) => "MISSING (required)",
                    (false, false) => "missing (optional)",
                };
                lines.push(format!("- {id}: {state}"));
            }
        }
    }
    let count = input.lines.clamp(1, MAX_LOG_LINES);
    lines.push(String::new());
    lines.push(format!(
        "### Logs (last {count} lines of each; paths, names and tokens removed)"
    ));
    for name in input.logs {
        lines.extend([
            String::new(),
            format!("#### {name}"),
            String::new(),
            "```".to_owned(),
        ]);
        match tail(&logs_directory(input.data).join(name), count) {
            Ok(tail) if tail.is_empty() => lines.push("(empty)".to_owned()),
            Ok(tail) => lines.extend(tail.iter().map(|line| clip(&input.redactor.redact(line)))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                lines.push("(no file)".to_owned());
            }
            // The kind only: an I/O message can carry a path.
            Err(error) => lines.push(format!("(unreadable: {:?})", error.kind())),
        }
        lines.push("```".to_owned());
    }
    lines.push(String::new());
    lines.join("\n")
}

fn clip(line: &str) -> String {
    if line.chars().count() <= LINE_CHARS {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(LINE_CHARS).collect();
    cut.push_str("...");
    cut
}

/// The last `count` lines of the log at `path`, then of the file rotation made of it before
/// (`<path>.1`) when the live file holds fewer. Reads only the end of each file.
fn tail(path: &Path, count: usize) -> io::Result<Vec<String>> {
    let mut lines = last_lines(path, count)?;
    if lines.len() < count {
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        // A log that never rotated has no older file, and that is no failure.
        if let Ok(mut older) = last_lines(Path::new(&rotated), count - lines.len()) {
            older.append(&mut lines);
            lines = older;
        }
    }
    Ok(lines)
}

fn last_lines(path: &Path, count: usize) -> io::Result<Vec<String>> {
    let mut file = OpenOptions::new().read(true).open_no_follow(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    if start > 0 {
        // The first line of a cut read is the end of a longer one.
        lines.next();
    }
    let all: Vec<&str> = lines.collect();
    let from = all.len().saturating_sub(count);
    Ok(all[from..].iter().map(|line| (*line).to_owned()).collect())
}

#[cfg(test)]
mod tests;
