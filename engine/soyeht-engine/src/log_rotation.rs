//! A size bound for the engine log.
//!
//! The launch agent points the engine's stderr at `engine.log` and nothing
//! else ever trims it, so one runaway loop could fill the disk. When stderr is
//! a regular file the engine now puts a pipe in its place and a thread copies
//! the lines into a [`RotatingFile`] at the same path: at most `keep + 1`
//! files of about `max_bytes` each, rotated only between lines so every record
//! stays a whole JSON line.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, PipeWriter, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

/// Size at which `engine.log` rotates.
pub const LOG_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Rotated files kept next to the live one (`engine.log.1` to `engine.log.4`).
pub const LOG_KEEP_ROTATED: usize = 4;
/// A single log line is cut here, so one record cannot defeat the size bound.
const LINE_MAX_BYTES: usize = 64 * 1024;
const READ_CHUNK_BYTES: usize = 16 * 1024;
const FLUSH_GRACE: Duration = Duration::from_secs(2);

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

    /// Writes whole lines. The file rotates before a line that would pass the
    /// cap, never in the middle of one.
    pub fn write_lines(&mut self, lines: &[u8]) -> io::Result<()> {
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

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// Copies `source` into `log` line by line until it ends. A line longer than
/// `LINE_MAX_BYTES` is cut and terminated; a final line without a newline gets
/// one.
pub fn pump(mut source: impl Read, log: &mut RotatingFile) -> io::Result<()> {
    let mut chunk = vec![0_u8; READ_CHUNK_BYTES];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let count = match source.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        pending.extend_from_slice(&chunk[..count]);
        if let Some(end) = pending.iter().rposition(|byte| *byte == b'\n') {
            let rest = pending.split_off(end + 1);
            log.write_lines(&bounded_lines(&pending))?;
            pending = rest;
        }
        if pending.len() > LINE_MAX_BYTES {
            pending.push(b'\n');
            log.write_lines(&bounded_lines(&pending))?;
            pending.clear();
        }
    }
    if !pending.is_empty() {
        pending.push(b'\n');
        log.write_lines(&bounded_lines(&pending))?;
    }
    Ok(())
}

/// `lines` with every line cut to `LINE_MAX_BYTES`.
fn bounded_lines(lines: &[u8]) -> Vec<u8> {
    let mut bounded = Vec::with_capacity(lines.len());
    for line in lines.split_inclusive(|byte| *byte == b'\n') {
        if line.len() > LINE_MAX_BYTES {
            bounded.extend_from_slice(&line[..LINE_MAX_BYTES]);
            bounded.push(b'\n');
        } else {
            bounded.extend_from_slice(line);
        }
    }
    bounded
}

#[cfg(unix)]
#[path = "log_rotation/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "log_rotation/windows.rs"]
mod system;

/// Keeps stderr on the pipe while alive. Dropping it puts the original file
/// back on stderr and waits briefly for the copy thread to drain the pipe.
pub struct StderrRotation {
    original: system::Original,
    writer: Option<PipeWriter>,
    finished: mpsc::Receiver<()>,
}

/// Replaces stderr with a rotating copy when stderr is a regular file.
/// Returns `None` when it is not (a terminal or a pipe needs no bound) or when
/// the platform cannot name the file behind a descriptor.
pub fn capture_stderr(max_bytes: u64, keep: usize) -> io::Result<Option<StderrRotation>> {
    let original = system::current_stderr()?;
    let target = File::from(original.try_clone()?);
    if !target.metadata()?.is_file() {
        return Ok(None);
    }
    let Some(path) = system::descriptor_path(&target) else {
        return Ok(None);
    };
    let mut log = RotatingFile::open(&path, max_bytes, keep)?;
    let (reader, writer) = io::pipe()?;
    system::redirect_stderr(&writer)?;
    let (done, finished) = mpsc::channel();
    thread::Builder::new()
        .name("engine-log".into())
        .spawn(move || {
            // Nowhere to report: stderr is this pipe. The sender only wakes a waiting drop.
            let _ = pump(reader, &mut log);
            let _ = done.send(());
        })?;
    Ok(Some(StderrRotation {
        original,
        writer: Some(writer),
        finished,
    }))
}

impl Drop for StderrRotation {
    fn drop(&mut self) {
        // Shutdown path: if stderr cannot be restored or the flush times out, the process
        // is exiting and there is no better place to log it.
        let _ = system::restore_stderr(&self.original);
        // With stderr restored, this is the last writer: closing it ends the pipe.
        drop(self.writer.take());
        let _ = self.finished.recv_timeout(FLUSH_GRACE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;
    use uuid::Uuid;

    fn directory() -> PathBuf {
        let directory = std::env::temp_dir().join(format!("soyeht-log-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn records(count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for index in 0..count {
            let record = json!({"event":"engine.test","index":index,"pad":"x".repeat(80)});
            bytes.extend_from_slice(record.to_string().as_bytes());
            bytes.push(b'\n');
        }
        bytes
    }

    fn sizes(directory: &Path) -> Vec<(String, u64)> {
        let mut found: Vec<_> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().len(),
                )
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_flood_stays_within_the_cap_and_keeps_whole_json_lines() {
        let directory = directory();
        let path = directory.join("engine.log");
        let mut log = RotatingFile::open(&path, 10_000, 3).unwrap();
        // 20000 records is about 2 MB, two hundred times the cap.
        pump(Cursor::new(records(20_000)), &mut log).unwrap();

        let files = sizes(&directory);
        assert_eq!(files.len(), 4, "{files:?}");
        for (name, size) in &files {
            assert!(*size <= 10_000, "{name} is {size} bytes");
            let text = fs::read_to_string(directory.join(name)).unwrap();
            for line in text.lines() {
                let record: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(record["event"], "engine.test");
            }
        }
        // The newest records are in the live file.
        let live = fs::read_to_string(&path).unwrap();
        assert!(live.contains("\"index\":19999"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_log_that_is_already_over_the_cap_rotates_on_open() {
        let directory = directory();
        let path = directory.join("engine.log");
        fs::write(&path, records(500)).unwrap();
        let before = fs::metadata(&path).unwrap().len();
        let mut log = RotatingFile::open(&path, 10_000, 2).unwrap();
        log.write_lines(b"{\"event\":\"engine.after\"}\n").unwrap();

        assert_eq!(
            fs::metadata(directory.join("engine.log.1")).unwrap().len(),
            before
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"event\":\"engine.after\"}\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_endless_line_is_cut_instead_of_filling_the_disk() {
        let directory = directory();
        let path = directory.join("engine.log");
        let mut log = RotatingFile::open(&path, 1_000_000, 1).unwrap();
        let mut input = vec![b'a'; LINE_MAX_BYTES * 3 + 7];
        input.extend_from_slice(b"\nlast");
        pump(Cursor::new(input), &mut log).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("\nlast\n"));
        assert!(text.len() < LINE_MAX_BYTES * 4 + 64, "{} bytes", text.len());
        assert!(text.lines().all(|line| line.len() <= LINE_MAX_BYTES));
        fs::remove_dir_all(directory).unwrap();
    }

    const CHILD_MARKER: &str = "SOYEHT_LOG_ROTATION_CHILD";

    /// Does nothing on its own. `stderr_flood_is_bounded_end_to_end` runs it as
    /// a child with stderr on a file, the way the launch agent starts the
    /// engine.
    #[test]
    fn stderr_flood_child() {
        if std::env::var_os(CHILD_MARKER).is_none() {
            return;
        }
        let rotation = capture_stderr(10_000, 2).unwrap();
        assert!(rotation.is_some());
        for index in 0..5_000 {
            eprintln!("{}", json!({"event":"engine.child","index":index}));
        }
        drop(rotation);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn stderr_flood_is_bounded_end_to_end() {
        let directory = directory();
        let path = directory.join("engine.log");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "log_rotation::tests::stderr_flood_child",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD_MARKER, "1")
            .stdout(std::process::Stdio::null())
            .stderr(File::create(&path).unwrap())
            .status()
            .unwrap();
        assert!(status.success());

        let files = sizes(&directory);
        assert_eq!(files.len(), 3, "{files:?}");
        assert!(files.iter().all(|(_, size)| *size <= 10_000), "{files:?}");
        let live = fs::read_to_string(&path).unwrap();
        assert!(live.contains("\"index\":4999"), "the last record is kept");
        fs::remove_dir_all(directory).unwrap();
    }
}
