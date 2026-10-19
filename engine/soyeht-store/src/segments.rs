use crate::{Error, Result};
use soyeht_platform::fs::{OpenOptionsPrivateExt, sync_directory};
use soyeht_protocol::limits;
use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// Makes the disk flush of a segment slow, as it is on a busy machine, so a test can show
/// what waits for it. Compiled only with `test-support`.
#[cfg(feature = "test-support")]
pub mod slow_disk {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static PAUSE_MS: AtomicU64 = AtomicU64::new(0);

    pub fn set(pause: Duration) {
        PAUSE_MS.store(
            u64::try_from(pause.as_millis()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub(super) fn wait() {
        let pause = PAUSE_MS.load(Ordering::Relaxed);
        if pause > 0 {
            std::thread::sleep(Duration::from_millis(pause));
        }
    }
}

/// Flushes a segment to the disk. On macOS this is a full flush of the drive, which takes
/// seconds when the machine writes a lot.
fn flush_to_disk(file: &File) -> io::Result<()> {
    #[cfg(feature = "test-support")]
    slow_disk::wait();
    file.sync_all()
}

/// Flushes the entries of a directory (created and removed segments).
fn flush_directory(directory: &Path) -> io::Result<()> {
    #[cfg(feature = "test-support")]
    slow_disk::wait();
    sync_directory(directory)
}

const MAGIC: &[u8; 8] = b"SOYPT001";
const HEADER: usize = 13;
const CHECKSUM: usize = 4;

/// One stretch of a pane's output. `B` is how the bytes are held: owned by default, or behind a
/// shared pointer when a caller keeps the record in memory and writes it to the log too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record<B = Vec<u8>> {
    Data { offset: u64, bytes: B },
    Gap { from: u64, to: u64 },
}
impl<B: Deref> Record<B>
where
    B::Target: AsRef<[u8]>,
{
    pub fn from(&self) -> u64 {
        match self {
            Self::Data { offset, .. } => *offset,
            Self::Gap { from, .. } => *from,
        }
    }
    pub fn end(&self) -> u64 {
        match self {
            Self::Data { offset, bytes } => offset.saturating_add((**bytes).as_ref().len() as u64),
            Self::Gap { to, .. } => *to,
        }
    }
}

struct Entry {
    file: u64,
    position: u64,
    from: u64,
    end: u64,
}
struct Segment {
    number: u64,
    size: u64,
}

pub struct Log {
    directory: PathBuf,
    segments: VecDeque<Segment>,
    index: VecDeque<Entry>,
    next: u64,
    /// Segments that hold records written but not yet flushed (see `append_unsynced`).
    unsynced: BTreeSet<u64>,
    /// A segment was created or removed since the directory was last flushed.
    directory_unsynced: bool,
    pub recovered_truncated_tail: bool,
}

/// The segment files whose newest records are written but not flushed. Flushing them takes as
/// long as the disk takes, so it is done off any lock that readers of the log share.
pub struct Unsynced {
    files: Vec<(u64, PathBuf)>,
    /// The log's directory, when a segment was created or removed since it was last flushed.
    directory: Option<PathBuf>,
}

impl Unsynced {
    pub fn flush(&self) -> Result<()> {
        for (_, path) in &self.files {
            match OpenOptions::new().write(true).open_no_follow(path) {
                Ok(file) => flush_to_disk(&file)?,
                // Retention removed the segment: there is nothing left to flush.
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(directory) = &self.directory {
            flush_directory(directory)?;
        }
        Ok(())
    }
}

fn filename(number: u64) -> String {
    format!("{number:016x}.pty")
}

impl Log {
    pub fn open(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let mut numbers = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".pty") {
                if stem.len() != 16 || !entry.file_type()?.is_file() {
                    return Err(Error::InvalidSegmentEntry);
                }
                numbers.push(u64::from_str_radix(stem, 16).map_err(|_| Error::InvalidSegmentName)?);
            }
        }
        numbers.sort_unstable();
        let mut log = Self {
            directory: directory.into(),
            segments: VecDeque::new(),
            index: VecDeque::new(),
            next: 0,
            unsynced: BTreeSet::new(),
            directory_unsynced: false,
            recovered_truncated_tail: false,
        };
        for (index, number) in numbers.iter().enumerate() {
            let path = directory.join(filename(*number));
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open_no_follow(&path)?;
            let length = file.metadata()?.len();
            if length < MAGIC.len() as u64 && index + 1 == numbers.len() {
                drop(file);
                fs::remove_file(directory.join(filename(*number)))?;
                sync_directory(directory)?;
                log.recovered_truncated_tail = true;
                break;
            }
            let mut magic = [0; 8];
            file.read_exact(&mut magic)?;
            if &magic != MAGIC || length > limits::SEGMENT_BYTES as u64 {
                return Err(Error::InvalidSegmentHeader);
            }
            let mut position = MAGIC.len() as u64;
            while position < length {
                let record = match read_record(&mut file) {
                    Ok(record) => record,
                    Err(Error::Io(error))
                        if error.kind() == ErrorKind::UnexpectedEof
                            && index + 1 == numbers.len() =>
                    {
                        file.set_len(position)?;
                        file.sync_all()?;
                        log.recovered_truncated_tail = true;
                        break;
                    }
                    Err(error) => return Err(error),
                };
                if !log.index.is_empty() && record.from() != log.next {
                    return Err(Error::SegmentHole);
                }
                log.next = record.end();
                log.index.push_back(Entry {
                    file: *number,
                    position,
                    from: record.from(),
                    end: record.end(),
                });
                position = file.stream_position()?;
            }
            log.segments.push_back(Segment {
                number: *number,
                size: position,
            });
            file.sync_all()?;
        }
        Ok(log)
    }

    pub fn next_offset(&self) -> u64 {
        self.next
    }
    pub fn first_offset(&self) -> u64 {
        self.index
            .front()
            .map(|entry| entry.from)
            .unwrap_or(self.next)
    }

    pub fn align_forward(&self, offset: u64) -> u64 {
        self.index
            .iter()
            .find(|entry| entry.from >= offset)
            .map(|entry| entry.from)
            .unwrap_or(self.next)
    }

    /// Writes the record and flushes it to the disk before returning.
    pub fn append(&mut self, record: Record) -> Result<()> {
        self.write(record, true)
    }

    /// Writes the record and returns without flushing it. The record is readable at once;
    /// `take_unsynced` hands out the flush that makes it survive a power loss.
    pub fn append_unsynced(&mut self, record: Record) -> Result<()> {
        self.write(record, false)
    }

    /// The flush owed for the records `append_unsynced` wrote, once.
    pub fn take_unsynced(&mut self) -> Option<Unsynced> {
        if self.unsynced.is_empty() && !self.directory_unsynced {
            return None;
        }
        let files = std::mem::take(&mut self.unsynced)
            .into_iter()
            .map(|number| (number, self.directory.join(filename(number))))
            .collect();
        let directory =
            std::mem::take(&mut self.directory_unsynced).then(|| self.directory.clone());
        Some(Unsynced { files, directory })
    }

    /// Owes the flush again, when the one `take_unsynced` handed out failed.
    pub fn restore_unsynced(&mut self, unsynced: &Unsynced) {
        self.directory_unsynced |= unsynced.directory.is_some();
        for (number, _) in &unsynced.files {
            if self
                .segments
                .iter()
                .any(|segment| segment.number == *number)
            {
                self.unsynced.insert(*number);
            }
        }
    }

    fn write(&mut self, record: Record, flush: bool) -> Result<()> {
        if record.from() != self.next || record.end() <= record.from() {
            return Err(Error::InvalidTerminalRecord);
        }
        let encoded = encode(&record)?;
        let need_new = self.segments.back().is_none_or(|segment| {
            segment.size + encoded.len() as u64 > limits::SEGMENT_BYTES as u64
        });
        if need_new {
            let number = self.segments.back().map(|s| s.number + 1).unwrap_or(0);
            let path = self.directory.join(filename(number));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .owner_only()
                .open(path)?;
            file.write_all(MAGIC)?;
            if flush {
                flush_to_disk(&file)?;
                flush_directory(&self.directory)?;
                self.directory_unsynced = false;
            } else {
                // The flush of the segment and of the directory is owed with the records.
                self.directory_unsynced = true;
            }
            self.segments.push_back(Segment {
                number,
                size: MAGIC.len() as u64,
            });
        }
        let segment = self.segments.back_mut().ok_or(Error::SegmentIndexMissing)?;
        let mut file = OpenOptions::new()
            .write(true)
            .open_no_follow(&self.directory.join(filename(segment.number)))?;
        file.set_len(segment.size)?;
        file.seek(SeekFrom::Start(segment.size))?;
        file.write_all(&encoded)?;
        if flush {
            // One flush covers every earlier write to this file.
            flush_to_disk(&file)?;
            self.unsynced.remove(&segment.number);
        } else {
            self.unsynced.insert(segment.number);
        }
        self.index.push_back(Entry {
            file: segment.number,
            position: segment.size,
            from: record.from(),
            end: record.end(),
        });
        segment.size += encoded.len() as u64;
        self.next = record.end();
        let mut total: u64 = self.segments.iter().map(|s| s.size).sum();
        while total > limits::PANE_RETENTION_BYTES as u64 && self.segments.len() > 1 {
            let oldest_number = self
                .segments
                .front()
                .ok_or(Error::SegmentIndexMissing)?
                .number;
            fs::remove_file(self.directory.join(filename(oldest_number)))?;
            self.unsynced.remove(&oldest_number);
            if flush {
                flush_directory(&self.directory)?;
                self.directory_unsynced = false;
            } else {
                self.directory_unsynced = true;
            }
            let oldest = self
                .segments
                .pop_front()
                .ok_or(Error::SegmentRetentionLost)?;
            total -= oldest.size;
            while self
                .index
                .front()
                .is_some_and(|entry| entry.file == oldest.number)
            {
                self.index.pop_front();
            }
        }
        Ok(())
    }

    pub fn read(&self, from: u64, to: u64, maximum: usize) -> Result<Vec<Record>> {
        if from < self.first_offset() || from > to || to > self.next {
            return Err(Error::CursorOutOfRange);
        }
        let mut output = Vec::new();
        let mut remaining = maximum;
        for entry in self.index.iter().filter(|e| e.end > from && e.from < to) {
            if remaining == 0 || output.len() == 256 {
                break;
            }
            let mut file = File::open(self.directory.join(filename(entry.file)))?;
            file.seek(SeekFrom::Start(entry.position))?;
            match read_record(&mut file)? {
                Record::Data { offset, bytes } => {
                    let start = from.saturating_sub(offset) as usize;
                    let end = bytes
                        .len()
                        .min((to - offset) as usize)
                        .min(start.saturating_add(remaining));
                    remaining -= end - start;
                    output.push(Record::Data {
                        offset: offset + start as u64,
                        bytes: bytes[start..end].to_vec(),
                    });
                }
                Record::Gap {
                    from: gap_from,
                    to: gap_to,
                } => output.push(Record::Gap {
                    from: from.max(gap_from),
                    to: to.min(gap_to),
                }),
            }
        }
        Ok(output)
    }
}

fn encode(record: &Record) -> Result<Vec<u8>> {
    let (kind, payload) = match record {
        Record::Data { offset, bytes }
            if bytes.len() <= limits::TERMINAL_PAYLOAD_BYTES
                && offset.checked_add(bytes.len() as u64).is_some() =>
        {
            (0, bytes.clone())
        }
        Record::Gap { to, .. } => (1, to.to_be_bytes().to_vec()),
        _ => return Err(Error::TerminalRecordTooLarge),
    };
    let mut bytes = Vec::with_capacity(HEADER + payload.len() + CHECKSUM);
    bytes.push(kind);
    bytes.extend_from_slice(&record.from().to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&crc32fast::hash(&bytes).to_be_bytes());
    Ok(bytes)
}

fn read_record(reader: &mut File) -> Result<Record> {
    let mut header = [0; HEADER];
    reader.read_exact(&mut header)?;
    let offset = u64::from_be_bytes(
        header[1..9]
            .try_into()
            .map_err(|_| Error::MalformedRecordOffset)?,
    );
    let length = u32::from_be_bytes(
        header[9..13]
            .try_into()
            .map_err(|_| Error::MalformedRecordLength)?,
    ) as usize;
    if length > limits::TERMINAL_PAYLOAD_BYTES || (header[0] == 1 && length != 8) || header[0] > 1 {
        return Err(Error::InvalidRecordLayout);
    }
    if header[0] == 0 && offset.checked_add(length as u64).is_none() {
        return Err(Error::TerminalOffsetOverflow);
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    let mut checksum = [0; CHECKSUM];
    reader.read_exact(&mut checksum)?;
    let mut digest = crc32fast::Hasher::new();
    digest.update(&header);
    digest.update(&payload);
    if digest.finalize() != u32::from_be_bytes(checksum) {
        return Err(Error::SegmentChecksumMismatch);
    }
    let record = if header[0] == 0 {
        Record::Data {
            offset,
            bytes: payload,
        }
    } else {
        Record::Gap {
            from: offset,
            to: u64::from_be_bytes(payload.try_into().map_err(|_| Error::MalformedGap)?),
        }
    };
    if record.end() <= record.from() {
        return Err(Error::InvalidRecordInterval);
    }
    Ok(record)
}
