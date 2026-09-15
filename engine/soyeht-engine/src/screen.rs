use crate::errors::{self, Result};
use soyeht_protocol::limits;
use soyeht_store::segments::{Log, Record};
use std::{collections::VecDeque, path::Path};

pub struct Screen {
    disk: Log,
    live: VecDeque<Record>,
    live_bytes: usize,
    next: u64,
    pub recording: bool,
}

impl Screen {
    pub fn open(directory: &Path, recording: bool) -> Result<Self> {
        let disk = Log::open(directory)?;
        Ok(Self {
            next: disk.next_offset(),
            disk,
            live: VecDeque::new(),
            live_bytes: 0,
            recording,
        })
    }
    pub fn next_offset(&self) -> u64 {
        self.next
    }
    pub fn first_offset(&self) -> u64 {
        self.disk
            .first_offset()
            .min(self.live.front().map(Record::from).unwrap_or(self.next))
    }

    pub fn append(&mut self, record: Record) -> Result<()> {
        if record.from() != self.next {
            return Err(errors::internal("Noncontiguous screen record"));
        }
        if self.recording {
            if self.disk.next_offset() < record.from() {
                self.disk.append(Record::Gap {
                    from: self.disk.next_offset(),
                    to: record.from(),
                })?;
            }
            self.disk.append(record.clone())?;
        }
        self.next = record.end();
        if let Record::Data { bytes, .. } = &record {
            self.live_bytes += bytes.len();
        }
        self.live.push_back(record);
        while self.live_bytes > limits::TAIL_BYTES || self.live.len() > 1024 {
            if let Some(Record::Data { bytes, .. }) = self.live.pop_front() {
                self.live_bytes -= bytes.len();
            }
        }
        Ok(())
    }

    pub fn append_data(&mut self, offset: u64, bytes: Vec<u8>) -> Result<()> {
        if offset != self.next {
            return Err(errors::internal("Noncontiguous screen bytes"));
        }
        for chunk in bytes.chunks(limits::TERMINAL_PAYLOAD_BYTES) {
            self.append(Record::Data {
                offset: self.next,
                bytes: chunk.to_vec(),
            })?;
        }
        Ok(())
    }

    pub fn tail_start(&self) -> u64 {
        let desired = self
            .next
            .saturating_sub(limits::TAIL_BYTES as u64)
            .max(self.first_offset());
        let live_first = self.live.front().map(Record::from).unwrap_or(self.next);
        if desired < live_first {
            self.disk.align_forward(desired).min(live_first)
        } else {
            self.live
                .iter()
                .find(|record| record.from() >= desired)
                .map(Record::from)
                .unwrap_or(self.next)
        }
    }

    pub fn read(&self, from: u64, to: u64, maximum: usize) -> Result<Vec<Record>> {
        if from < self.first_offset() || from > to || to > self.next {
            return Err(errors::internal("Screen cursor outside retained range"));
        }
        let mut result = Vec::new();
        let mut position = from;
        let mut remaining = maximum;
        let live_first = self.live.front().map(Record::from).unwrap_or(self.next);
        if position < live_first && position < self.disk.next_offset() {
            for record in self.disk.read(
                position,
                to.min(live_first).min(self.disk.next_offset()),
                remaining,
            )? {
                if let Record::Data { bytes, .. } = &record {
                    remaining -= bytes.len();
                }
                position = record.end();
                result.push(record);
            }
        }
        if remaining == 0 || position == to {
            return Ok(result);
        }
        if position < live_first {
            let end = to.min(live_first);
            result.push(Record::Gap {
                from: position,
                to: end,
            });
            position = end;
        }
        for record in &self.live {
            if record.end() <= position || record.from() >= to {
                continue;
            }
            if remaining == 0 {
                break;
            }
            let record = match record {
                Record::Data { offset, bytes } => {
                    let start = position.saturating_sub(*offset) as usize;
                    let end = bytes
                        .len()
                        .min((to - offset) as usize)
                        .min(start + remaining);
                    remaining -= end - start;
                    Record::Data {
                        offset: offset + start as u64,
                        bytes: bytes[start..end].to_vec(),
                    }
                }
                Record::Gap { from, to: end } => Record::Gap {
                    from: position.max(*from),
                    to: to.min(*end),
                },
            };
            position = record.end();
            result.push(record);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn disabled_recording_does_not_persist_terminal_bytes() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        screen
            .append(Record::Data {
                offset: 0,
                bytes: b"saved".to_vec(),
            })
            .unwrap();
        screen.recording = false;
        screen
            .append(Record::Data {
                offset: 5,
                bytes: b"private".to_vec(),
            })
            .unwrap();
        assert_eq!(
            screen.read(5, 12, 64).unwrap(),
            vec![Record::Data {
                offset: 5,
                bytes: b"private".to_vec()
            }]
        );
        screen.recording = true;
        screen
            .append(Record::Data {
                offset: 12,
                bytes: b"again".to_vec(),
            })
            .unwrap();
        drop(screen);
        let screen = Screen::open(&path, true).unwrap();
        assert_eq!(
            screen.read(5, 17, 64).unwrap(),
            vec![
                Record::Gap { from: 5, to: 12 },
                Record::Data {
                    offset: 12,
                    bytes: b"again".to_vec()
                }
            ]
        );
        for file in fs::read_dir(&path).unwrap() {
            assert!(
                !fs::read(file.unwrap().path())
                    .unwrap()
                    .windows(7)
                    .any(|bytes| bytes == b"private")
            );
        }
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn append_data_splits_bytes_at_the_terminal_record_limit() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        let bytes = vec![b'x'; limits::TERMINAL_PAYLOAD_BYTES + 17];
        screen.append_data(0, bytes.clone()).unwrap();
        let records = screen.read(0, bytes.len() as u64, bytes.len()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records
                .into_iter()
                .flat_map(|record| match record {
                    Record::Data { bytes, .. } => bytes,
                    Record::Gap { .. } => Vec::new(),
                })
                .collect::<Vec<_>>(),
            bytes
        );
        fs::remove_dir_all(path).unwrap();
    }
}
