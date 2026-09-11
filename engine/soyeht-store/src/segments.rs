use soyeht_protocol::limits;
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"SOYPT001";
const HEADER: usize = 13;
const CHECKSUM: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Data { offset: u64, bytes: Vec<u8> },
    Gap { from: u64, to: u64 },
}
impl Record {
    pub fn from(&self) -> u64 {
        match self {
            Self::Data { offset, .. } => *offset,
            Self::Gap { from, .. } => *from,
        }
    }
    pub fn end(&self) -> u64 {
        match self {
            Self::Data { offset, bytes } => offset.saturating_add(bytes.len() as u64),
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
    pub recovered_truncated_tail: bool,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn filename(number: u64) -> String {
    format!("{number:016x}.pty")
}

impl Log {
    pub fn open(directory: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let mut numbers = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".pty") {
                if stem.len() != 16 || !entry.file_type()?.is_file() {
                    return Err(invalid("invalid segment filename or type"));
                }
                numbers.push(
                    u64::from_str_radix(stem, 16)
                        .map_err(|_| invalid("invalid segment filename"))?,
                );
            }
        }
        numbers.sort_unstable();
        let mut log = Self {
            directory: directory.into(),
            segments: VecDeque::new(),
            index: VecDeque::new(),
            next: 0,
            recovered_truncated_tail: false,
        };
        for (index, number) in numbers.iter().enumerate() {
            let path = directory.join(filename(*number));
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?;
            let length = file.metadata()?.len();
            if length < MAGIC.len() as u64 && index + 1 == numbers.len() {
                drop(file);
                fs::remove_file(directory.join(filename(*number)))?;
                File::open(directory)?.sync_all()?;
                log.recovered_truncated_tail = true;
                break;
            }
            let mut magic = [0; 8];
            file.read_exact(&mut magic)?;
            if &magic != MAGIC || length > limits::SEGMENT_BYTES as u64 {
                return Err(invalid("invalid segment header or size"));
            }
            let mut position = MAGIC.len() as u64;
            while position < length {
                let record = match read_record(&mut file) {
                    Ok(record) => record,
                    Err(error)
                        if error.kind() == io::ErrorKind::UnexpectedEof
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
                    return Err(invalid("unmarked hole in terminal segments"));
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

    pub fn append(&mut self, record: Record) -> io::Result<()> {
        if record.from() != self.next || record.end() <= record.from() {
            return Err(invalid("noncontiguous or empty terminal record"));
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
                .mode(0o600)
                .open(path)?;
            file.write_all(MAGIC)?;
            file.sync_all()?;
            File::open(&self.directory)?.sync_all()?;
            self.segments.push_back(Segment {
                number,
                size: MAGIC.len() as u64,
            });
        }
        let segment = self.segments.back_mut().expect("active segment");
        let mut file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.directory.join(filename(segment.number)))?;
        file.set_len(segment.size)?;
        file.seek(SeekFrom::Start(segment.size))?;
        file.write_all(&encoded)?;
        file.sync_all()?;
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
            let oldest = self.segments.front().expect("oldest segment");
            fs::remove_file(self.directory.join(filename(oldest.number)))?;
            File::open(&self.directory)?.sync_all()?;
            let oldest = self.segments.pop_front().expect("oldest segment");
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

    pub fn read(&self, from: u64, to: u64, maximum: usize) -> io::Result<Vec<Record>> {
        if from < self.first_offset() || from > to || to > self.next {
            return Err(invalid("terminal cursor outside retained range"));
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

fn encode(record: &Record) -> io::Result<Vec<u8>> {
    let (kind, payload) = match record {
        Record::Data { offset, bytes }
            if bytes.len() <= limits::TERMINAL_PAYLOAD_BYTES
                && offset.checked_add(bytes.len() as u64).is_some() =>
        {
            (0, bytes.clone())
        }
        Record::Gap { to, .. } => (1, to.to_be_bytes().to_vec()),
        _ => return Err(invalid("terminal record too large")),
    };
    let mut bytes = Vec::with_capacity(HEADER + payload.len() + CHECKSUM);
    bytes.push(kind);
    bytes.extend_from_slice(&record.from().to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&crc32fast::hash(&bytes).to_be_bytes());
    Ok(bytes)
}

fn read_record(reader: &mut File) -> io::Result<Record> {
    let mut header = [0; HEADER];
    reader.read_exact(&mut header)?;
    let offset = u64::from_be_bytes(header[1..9].try_into().expect("fixed header"));
    let length = u32::from_be_bytes(header[9..13].try_into().expect("fixed header")) as usize;
    if length > limits::TERMINAL_PAYLOAD_BYTES || (header[0] == 1 && length != 8) || header[0] > 1 {
        return Err(invalid("invalid terminal record length or type"));
    }
    if header[0] == 0 && offset.checked_add(length as u64).is_none() {
        return Err(invalid("terminal offset overflow"));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    let mut checksum = [0; CHECKSUM];
    reader.read_exact(&mut checksum)?;
    let mut digest = crc32fast::Hasher::new();
    digest.update(&header);
    digest.update(&payload);
    if digest.finalize() != u32::from_be_bytes(checksum) {
        return Err(invalid("terminal record checksum mismatch"));
    }
    let record = if header[0] == 0 {
        Record::Data {
            offset,
            bytes: payload,
        }
    } else {
        Record::Gap {
            from: offset,
            to: u64::from_be_bytes(payload.try_into().expect("validated gap length")),
        }
    };
    if record.end() <= record.from() {
        return Err(invalid("invalid terminal record interval"));
    }
    Ok(record)
}
