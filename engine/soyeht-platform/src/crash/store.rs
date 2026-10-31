//! Where the reports are kept: `crashes/` of the profile's data directory.
//!
//! At most [`MAX_REPORTS`] reports; the oldest is removed when a new one comes. A report is
//! pending until the person has seen it: `acknowledged` lists the names of the reports that were.

use super::report::{MAX_REPORT_BYTES, Report};
use crate::fs::{
    OpenOptionsPrivateExt as _, WriteMode, publish_new, set_private_dir, write_atomic,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read as _},
    path::{Path, PathBuf},
};

/// The folder of the reports, inside the data directory.
const DIRECTORY: &str = "crashes";
/// Holds the name of the newest report the person has seen.
const ACKNOWLEDGED: &str = "acknowledged";
const PREFIX: &str = "crash-";
const SUFFIX: &str = ".txt";

/// How many reports of one program in one second keep their own file; a program writes few.
const SAME_SECOND: usize = 10;

/// Most reports kept.
pub const MAX_REPORTS: usize = 8;

pub fn directory(data: &Path) -> PathBuf {
    data.join(DIRECTORY)
}

/// A report on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The file name, which sorts by time.
    pub name: String,
    pub report: Report,
}

/// `crash-<time, 12 digits>-<program>-<pid>-<n>.txt`: it sorts by time, and `n` tells apart the
/// reports one program writes in the same second.
fn file_name(report: &Report, n: usize) -> String {
    format!(
        "{PREFIX}{:012}-{}-{}-{n:02}{SUFFIX}",
        report.time, report.program, report.pid
    )
}

fn is_report_name(name: &str) -> bool {
    name.starts_with(PREFIX) && name.ends_with(SUFFIX)
}

/// Writes `report` and removes the oldest reports beyond [`MAX_REPORTS`]. A name that is taken
/// (two reports of one program in one second) is never replaced: the next `n` is tried.
pub fn write(data: &Path, report: &Report) -> io::Result<()> {
    let folder = directory(data);
    fs::create_dir_all(&folder)?;
    set_private_dir(&folder)?;
    let text = report.to_text();
    let mut taken = io::Error::from(io::ErrorKind::AlreadyExists);
    for n in 0..=SAME_SECOND {
        match publish_new(
            &folder.join(file_name(report, n)),
            text.as_bytes(),
            WriteMode::OwnerOnly,
        ) {
            Ok(()) => return prune(&folder),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => taken = error,
            Err(error) => return Err(error),
        }
    }
    Err(taken)
}

fn names(folder: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(folder)? {
        // A name that is not text is not one of ours.
        if let Ok(name) = entry?.file_name().into_string()
            && is_report_name(&name)
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn prune(folder: &Path) -> io::Result<()> {
    let names = names(folder)?;
    for name in &names[..names.len().saturating_sub(MAX_REPORTS)] {
        // Another process may have pruned it at the same time.
        match fs::remove_file(folder.join(name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// The report names, or none when the folder is not there.
fn names_if_any(folder: &Path) -> io::Result<Vec<String>> {
    match names(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        other => other,
    }
}

/// The reports on disk, oldest first. A folder that is not there holds none; a file that is not a
/// report of this format, or is too large, is left out.
pub fn list(data: &Path) -> io::Result<Vec<Stored>> {
    let folder = directory(data);
    Ok(names_if_any(&folder)?
        .into_iter()
        .filter_map(|name| {
            let report = read(&folder.join(&name))?;
            Some(Stored { name, report })
        })
        .collect())
}

fn read(path: &Path) -> Option<Report> {
    let mut text = String::new();
    // ignored: a file nobody can read, or that is not text, is not a report; it is left out.
    let file = OpenOptions::new().read(true).open_no_follow(path).ok()?;
    let limit = u64::try_from(MAX_REPORT_BYTES * 2 + 1).unwrap_or(u64::MAX);
    // ignored: the same as above, for a file that is not text.
    file.take(limit).read_to_string(&mut text).ok()?;
    Report::parse(&text)
}

/// The reports the person has not seen and that ended a program of Soyeht: what the notice
/// "Soyeht closed unexpectedly" is about. A report is seen when its exact name is in
/// `acknowledged`, so a clock that steps back, or two programs that crash in one second, cannot
/// hide one.
pub fn pending(data: &Path) -> io::Result<Vec<Stored>> {
    let seen = acknowledged(data)?;
    Ok(list(data)?
        .into_iter()
        .filter(|stored| stored.report.fatal && !seen.contains(&stored.name))
        .collect())
}

fn acknowledged(data: &Path) -> io::Result<Vec<String>> {
    match fs::read_to_string(directory(data).join(ACKNOWLEDGED)) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// Marks the reports named in `shown` as seen, or every pending report when `shown` is empty.
/// What the person saw is what is acknowledged: a report that came after the notice was drawn is
/// still pending. Returns how many were pending. The names of reports that are gone are dropped
/// from the list.
pub fn acknowledge(data: &Path, shown: &[String]) -> io::Result<usize> {
    let pending: Vec<String> = pending(data)?
        .into_iter()
        .map(|stored| stored.name)
        .collect();
    let counted: Vec<&String> = pending
        .iter()
        .filter(|name| shown.is_empty() || shown.contains(name))
        .collect();
    let folder = directory(data);
    let existing = names_if_any(&folder)?;
    let mut seen = acknowledged(data)?;
    seen.extend(counted.iter().map(|name| (*name).clone()));
    seen.retain(|name| existing.contains(name));
    seen.sort();
    seen.dedup();
    if !existing.is_empty() {
        write_atomic(
            &folder.join(ACKNOWLEDGED),
            seen.join("\n").as_bytes(),
            WriteMode::OwnerOnly,
        )?;
    }
    Ok(counted.len())
}
