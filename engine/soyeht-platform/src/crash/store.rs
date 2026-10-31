//! Where the reports are kept: `crashes/` of the profile's data directory.
//!
//! At most [`MAX_REPORTS`] reports; the oldest is removed when a new one comes. A report is
//! pending until the person has seen it: `acknowledged` holds the name of the newest report
//! that was.

use super::report::{MAX_REPORT_BYTES, Report};
use crate::fs::{OpenOptionsPrivateExt as _, WriteMode, set_private_dir, write_atomic};
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

/// Writes `report` and removes the oldest reports beyond [`MAX_REPORTS`].
pub fn write(data: &Path, report: &Report) -> io::Result<()> {
    let folder = directory(data);
    fs::create_dir_all(&folder)?;
    set_private_dir(&folder)?;
    let target = (0..SAME_SECOND)
        .map(|n| folder.join(file_name(report, n)))
        .find(|path| !path.exists())
        .unwrap_or_else(|| folder.join(file_name(report, SAME_SECOND)));
    write_atomic(&target, report.to_text().as_bytes(), WriteMode::OwnerOnly)?;
    prune(&folder)
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
        fs::remove_file(folder.join(name))?;
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
/// "Soyeht closed unexpectedly" is about.
pub fn pending(data: &Path) -> io::Result<Vec<Stored>> {
    let seen = acknowledged(data)?;
    Ok(list(data)?
        .into_iter()
        .filter(|stored| stored.report.fatal && stored.name > seen)
        .collect())
}

fn acknowledged(data: &Path) -> io::Result<String> {
    match fs::read_to_string(directory(data).join(ACKNOWLEDGED)) {
        Ok(name) => Ok(name.trim().to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// Marks every report on disk as seen. Returns how many were pending.
pub fn acknowledge(data: &Path) -> io::Result<usize> {
    let pending = pending(data)?.len();
    let folder = directory(data);
    if let Some(newest) = names_if_any(&folder)?.pop() {
        write_atomic(
            &folder.join(ACKNOWLEDGED),
            newest.as_bytes(),
            WriteMode::OwnerOnly,
        )?;
    }
    Ok(pending)
}
