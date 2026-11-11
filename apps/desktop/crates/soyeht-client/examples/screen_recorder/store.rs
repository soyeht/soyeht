//! Where the recordings go: `<root>/<date>/<pane>.jsonl`, in owner-only folders and files, with a
//! size cap on every file and on the run, so that a pane that floods the recorder cannot fill the
//! disk.
//!
//! A file that reaches its cap is renamed to `<pane>.<n>.jsonl` and a new `<pane>.jsonl` begins;
//! only the newest `keep` of those are kept. The recorder starts every file with the pane's header
//! and its whole screen, so each file can be read alone. Once the files this run holds reach the
//! total cap, records are dropped (and counted) instead of written.

use soyeht_platform::fs::{OpenOptionsPrivateExt, ensure_private_dir};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// A file is rotated before a record would take it past this.
    pub file_bytes: u64,
    /// How many rotated files of a pane are kept.
    pub keep: usize,
    /// What the files of this run may hold together.
    pub total_bytes: u64,
}

struct Segment {
    file: File,
    size: u64,
    /// Identifies this file among the ones this run opened.
    id: u64,
    /// Rotated files of this run, oldest first, with their sizes.
    rotated: VecDeque<(PathBuf, u64)>,
}

pub(crate) struct Store {
    root: PathBuf,
    limits: Limits,
    day: Option<String>,
    segments: HashMap<String, Segment>,
    opened: u64,
    /// Bytes of the files this run wrote that are still on disk.
    total: u64,
    dropped: u64,
}

impl Store {
    pub fn new(root: PathBuf, limits: Limits) -> io::Result<Self> {
        ensure_private_dir(&root)?;
        Ok(Self {
            root,
            limits,
            day: None,
            segments: HashMap::new(),
            opened: 0,
            total: 0,
            dropped: 0,
        })
    }

    /// Makes `pane`'s file ready for a record of `incoming` bytes written at `wall_ms`, rotating it
    /// or starting the next day's folder when that is due. Returns the identity of the file the
    /// record goes to (a record that follows one of another identity starts the file anew), or
    /// `None` when the run has reached its total cap and the record is dropped.
    pub fn prepare(
        &mut self,
        pane: &str,
        wall_ms: u64,
        incoming: usize,
    ) -> io::Result<Option<u64>> {
        let incoming = u64::try_from(incoming).unwrap_or(u64::MAX);
        if self.total.saturating_add(incoming) > self.limits.total_bytes {
            self.dropped += 1;
            return Ok(None);
        }
        let day = day_of(wall_ms);
        if self.day.as_deref() != Some(day.as_str()) {
            self.segments.clear();
            self.day = Some(day);
        }
        let full = self.segments.get(pane).is_some_and(|segment| {
            segment.size > 0 && segment.size + incoming > self.limits.file_bytes
        });
        if full {
            self.rotate(pane)?;
        }
        if !self.segments.contains_key(pane) {
            let segment = self.open(pane)?;
            self.segments.insert(pane.to_owned(), segment);
        }
        Ok(self.segments.get(pane).map(|segment| segment.id))
    }

    /// Appends one line to `pane`'s file, as one write, so a stop never leaves half a record.
    pub fn write(&mut self, pane: &str, line: &str) -> io::Result<()> {
        let Some(segment) = self.segments.get_mut(pane) else {
            return Err(io::Error::other("the pane's file is not open"));
        };
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        segment.file.write_all(&bytes)?;
        let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        segment.size += length;
        self.total += length;
        Ok(())
    }

    /// Closes the file of a pane that is gone.
    pub fn close(&mut self, pane: &str) {
        self.segments.remove(pane);
    }

    /// How many records were dropped because the run reached its total cap.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    fn directory(&self) -> io::Result<PathBuf> {
        let directory = self.root.join(self.day.as_deref().unwrap_or("undated"));
        ensure_private_dir(&directory)?;
        Ok(directory)
    }

    fn open(&mut self, pane: &str) -> io::Result<Segment> {
        let directory = self.directory()?;
        let name = file_name(pane);
        let path = directory.join(format!("{name}.jsonl"));
        // A file of an earlier run is never appended to: it is put aside as a rotated file.
        if path.exists() {
            let number = highest_rotation(&directory, &name) + 1;
            fs::rename(&path, directory.join(format!("{name}.{number}.jsonl")))?;
        }
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .owner_only()
            .open_no_follow(&path)?;
        self.opened += 1;
        Ok(Segment {
            file,
            size: 0,
            id: self.opened,
            rotated: VecDeque::new(),
        })
    }

    fn rotate(&mut self, pane: &str) -> io::Result<()> {
        let Some(old) = self.segments.remove(pane) else {
            return Ok(());
        };
        let directory = self.directory()?;
        let name = file_name(pane);
        let Segment {
            file,
            size,
            mut rotated,
            ..
        } = old;
        drop(file);
        let aside = directory.join(format!(
            "{name}.{}.jsonl",
            highest_rotation(&directory, &name) + 1
        ));
        fs::rename(directory.join(format!("{name}.jsonl")), &aside)?;
        rotated.push_back((aside, size));
        while rotated.len() > self.limits.keep {
            if let Some((oldest, length)) = rotated.pop_front() {
                fs::remove_file(oldest)?;
                self.total = self.total.saturating_sub(length);
            }
        }
        let mut next = self.open(pane)?;
        next.rotated = rotated;
        self.segments.insert(pane.to_owned(), next);
        Ok(())
    }
}

/// The UTC date of a time in milliseconds since the epoch, as `2026-11-11`.
fn day_of(wall_ms: u64) -> String {
    i64::try_from(wall_ms / 1000)
        .ok()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map_or_else(
            || "undated".to_owned(),
            |time| time.format("%Y-%m-%d").to_string(),
        )
}

/// A pane id as a file name: letters, digits, `-` and `_`, whatever the id holds.
fn file_name(pane: &str) -> String {
    let name: String = pane
        .chars()
        .take(100)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "pane".to_owned()
    } else {
        name
    }
}

/// The highest `n` of the `<name>.<n>.jsonl` files in `directory`, or 0.
fn highest_rotation(directory: &Path, name: &str) -> u64 {
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let number = file
                .strip_prefix(name)?
                .strip_prefix('.')?
                .strip_suffix(".jsonl")?;
            number.parse::<u64>().ok()
        })
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests;
