//! A log file with a size bound.
//!
//! At most `keep + 1` files of about `max_bytes` each, rotated only between lines so every
//! record stays a whole JSON line, and any single line cut at [`LINE_MAX_BYTES`]. The engine
//! copies its standard error into one (ADR 0082) and a program with no terminal writes its log
//! lines to one ([`super::to_file`]).

use crate::fs::OpenOptionsPrivateExt;
use std::{
    borrow::Cow,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Size at which a log file rotates.
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Rotated files kept next to the live one (`engine.log.1` to `engine.log.4`).
pub const KEEP_ROTATED: usize = 4;
/// A single log line is cut here, so one record cannot defeat the size bound.
pub const LINE_MAX_BYTES: usize = 64 * 1024;

pub struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: File,
    written: u64,
}

impl RotatingFile {
    /// Opens `path` for appending. A file that is already over `max_bytes`
    /// rotates first, so a log that grew before this existed stops growing.
    pub fn open(path: &Path, max_bytes: u64, keep: usize) -> io::Result<Self> {
        let file = append(path)?;
        let written = file.metadata()?.len();
        let mut log = Self {
            path: path.to_owned(),
            max_bytes,
            keep,
            file,
            written,
        };
        if log.written >= max_bytes {
            log.rotate()?;
        }
        Ok(log)
    }

    /// Writes whole lines, each cut to [`LINE_MAX_BYTES`]. The file rotates before a line
    /// that would pass the cap, never in the middle of one.
    pub fn write_lines(&mut self, lines: &[u8]) -> io::Result<()> {
        let lines = bounded_lines(lines);
        for line in lines.split_inclusive(|byte| *byte == b'\n') {
            let length = u64::try_from(line.len()).unwrap_or(u64::MAX);
            if self.written > 0 && self.written.saturating_add(length) > self.max_bytes {
                self.rotate()?;
            }
            self.file.write_all(line)?;
            self.written = self.written.saturating_add(length);
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if self.keep == 0 {
            self.file.set_len(0)?;
        } else {
            // Best effort: a missing oldest file is the normal case, and a real failure
            // surfaces in the final rename that this rotation must complete.
            let _ = fs::remove_file(self.numbered(self.keep));
            for index in (1..self.keep).rev() {
                // Best effort: a gap in the numbered files is normal.
                let _ = fs::rename(self.numbered(index), self.numbered(index + 1));
            }
            fs::rename(&self.path, self.numbered(1))?;
            self.file = append(&self.path)?;
        }
        self.written = 0;
        Ok(())
    }

    fn numbered(&self, index: usize) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".{index}"));
        PathBuf::from(name)
    }
}

/// Owner-only when this creates the file: a log can name paths.
fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .owner_only()
        .open(path)
}

/// `lines` with every line cut to `LINE_MAX_BYTES` and ended with a newline.
fn bounded_lines(lines: &[u8]) -> Cow<'_, [u8]> {
    if lines
        .split(|byte| *byte == b'\n')
        .all(|line| line.len() <= LINE_MAX_BYTES)
    {
        return Cow::Borrowed(lines);
    }
    let mut bounded = Vec::with_capacity(lines.len());
    for line in lines.split_inclusive(|byte| *byte == b'\n') {
        if line.len() > LINE_MAX_BYTES {
            bounded.extend_from_slice(&line[..LINE_MAX_BYTES]);
            bounded.push(b'\n');
        } else {
            bounded.extend_from_slice(line);
        }
    }
    Cow::Owned(bounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch_dir as scratch;

    /// `count` whole JSON lines, about a hundred bytes each.
    fn records(count: usize) -> Vec<u8> {
        let pad = "x".repeat(80);
        (0..count)
            .map(|index| format!("{{\"event\":\"test\",\"index\":{index},\"pad\":\"{pad}\"}}\n"))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn a_flood_stays_within_the_cap_and_keeps_whole_lines() {
        let directory = scratch("flood");
        let path = directory.join("a.log");
        let mut log = RotatingFile::open(&path, 10_000, 3).unwrap();
        // 20000 records is about 2 MB, two hundred times the cap.
        log.write_lines(&records(20_000)).unwrap();

        let files: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 4, "{files:?}");
        for file in &files {
            let size = fs::metadata(file).unwrap().len();
            assert!(size <= 10_000, "{file:?} is {size} bytes");
            let text = fs::read_to_string(file).unwrap();
            for line in text.lines() {
                assert!(line.starts_with("{\"event\":\"test\"") && line.ends_with('}'));
            }
        }
        // The newest records are in the live file.
        let live = fs::read_to_string(&path).unwrap();
        assert!(live.contains("\"index\":19999"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_log_that_is_already_over_the_cap_rotates_on_open() {
        let directory = scratch("over");
        let path = directory.join("a.log");
        fs::write(&path, records(500)).unwrap();
        let before = fs::metadata(&path).unwrap().len();
        let mut log = RotatingFile::open(&path, 10_000, 2).unwrap();
        log.write_lines(b"{\"event\":\"after\"}\n").unwrap();

        assert_eq!(
            fs::metadata(directory.join("a.log.1")).unwrap().len(),
            before
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"event\":\"after\"}\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_endless_line_is_cut_instead_of_filling_the_disk() {
        let directory = scratch("endless");
        let path = directory.join("a.log");
        let mut log = RotatingFile::open(&path, 1_000_000, 1).unwrap();
        let mut input = vec![b'a'; LINE_MAX_BYTES * 3 + 7];
        input.extend_from_slice(b"\nlast\n");
        log.write_lines(&input).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let longest = text.lines().map(str::len).max().unwrap();
        assert_eq!(longest, LINE_MAX_BYTES);
        assert_eq!(text.len(), LINE_MAX_BYTES + "\nlast\n".len());
        assert!(text.ends_with("\nlast\n"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_log_the_writer_creates_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let directory = scratch("private");
        let path = directory.join("a.log");
        drop(RotatingFile::open(&path, 10_000, 1).unwrap());
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
        fs::remove_dir_all(directory).unwrap();
    }
}
