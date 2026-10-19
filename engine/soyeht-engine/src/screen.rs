use crate::errors::{self, Result};
use soyeht_protocol::limits;
use soyeht_store::segments::{Log, Record, Unsynced};
use std::{collections::VecDeque, path::Path, sync::Arc};

/// A record kept in memory: the output bytes are shared with the supervisor, so a clone is cheap.
type LiveRecord = Record<Arc<Vec<u8>>>;

pub struct Screen {
    disk: Log,
    live: VecDeque<LiveRecord>,
    live_bytes: usize,
    next: u64,
    /// The supervisor's output position this screen has taken in and not yet told it to drop.
    unacknowledged: Option<u64>,
    pub recording: bool,
}

/// Output a screen holds that the supervisor may drop once its flush to the disk is done.
pub struct Durable {
    unsynced: Option<Unsynced>,
    /// The supervisor's output position the flush makes safe.
    pub through: u64,
}

impl Durable {
    /// Whether there is a flush to wait for. Output that was never written to the disk
    /// (recording is off) is safe to acknowledge at once.
    pub fn pending(&self) -> bool {
        self.unsynced.is_some()
    }

    /// Waits for the disk. Call it off the Core lock.
    pub fn flush(&self) -> Result<()> {
        match &self.unsynced {
            Some(unsynced) => unsynced.flush().map_err(errors::storage),
            None => Ok(()),
        }
    }
}

impl Screen {
    pub fn open(directory: &Path, recording: bool) -> Result<Self> {
        let disk = Log::open(directory)?;
        Ok(Self {
            next: disk.next_offset(),
            disk,
            live: VecDeque::new(),
            live_bytes: 0,
            unacknowledged: None,
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
        self.append_record(record, true)
    }

    /// Like `append`, without waiting for the disk: the terminal tick holds the Core lock, and
    /// a flush can take seconds. `take_durable` hands out the flush that is owed.
    pub fn append_deferred(&mut self, record: Record) -> Result<()> {
        self.append_record(record, false)
    }

    fn append_record(&mut self, record: Record, flush: bool) -> Result<()> {
        match record {
            Record::Data { offset, bytes } => {
                self.append_data_record(offset, Arc::new(bytes), flush)
            }
            Record::Gap { from, to } => self.append_gap(from, to, flush),
        }
    }

    pub fn append_shared_data_deferred(&mut self, offset: u64, bytes: Arc<Vec<u8>>) -> Result<()> {
        self.append_data_record(offset, bytes, false)
    }

    /// Notes that the supervisor's output through `position` is in this screen.
    pub fn take_in_through(&mut self, position: u64) {
        self.unacknowledged = Some(
            self.unacknowledged
                .map_or(position, |held| held.max(position)),
        );
    }

    /// The output waiting to be acknowledged and the flush it needs, once.
    pub fn take_durable(&mut self) -> Option<Durable> {
        let through = self.unacknowledged.take()?;
        Some(Durable {
            unsynced: self.disk.take_unsynced(),
            through,
        })
    }

    /// Owes `durable` again, when its flush failed: the next one retries it.
    pub fn restore_durable(&mut self, durable: &Durable) {
        self.take_in_through(durable.through);
        if let Some(unsynced) = &durable.unsynced {
            self.disk.restore_unsynced(unsynced);
        }
    }

    fn append_data_record(&mut self, offset: u64, bytes: Arc<Vec<u8>>, flush: bool) -> Result<()> {
        let record = LiveRecord::Data { offset, bytes };
        if record.from() != self.next {
            return Err(errors::internal("Noncontiguous screen record"));
        }
        if self.recording {
            self.bridge_disk_gap(record.from(), flush)?;
            let LiveRecord::Data { offset, bytes } = &record else {
                unreachable!()
            };
            self.disk_append(
                Record::Data {
                    offset: *offset,
                    bytes: bytes.as_ref().clone(),
                },
                flush,
            )?;
        }
        self.next = record.end();
        if let LiveRecord::Data { bytes, .. } = &record {
            self.live_bytes += bytes.len();
        }
        self.live.push_back(record);
        while self.live_bytes > limits::TAIL_BYTES || self.live.len() > 1024 {
            if let Some(LiveRecord::Data { bytes, .. }) = self.live.pop_front() {
                self.live_bytes -= bytes.len();
            }
        }
        Ok(())
    }

    /// Records the stretch the disk missed (while recording was off) before a record at `to`.
    fn bridge_disk_gap(&mut self, to: u64, flush: bool) -> Result<()> {
        let from = self.disk.next_offset();
        if from < to {
            self.disk_append(Record::Gap { from, to }, flush)?;
        }
        Ok(())
    }

    fn disk_append(&mut self, record: Record, flush: bool) -> Result<()> {
        Ok(if flush {
            self.disk.append(record)
        } else {
            self.disk.append_unsynced(record)
        }?)
    }

    fn append_gap(&mut self, from: u64, to: u64, flush: bool) -> Result<()> {
        let record = LiveRecord::Gap { from, to };
        if record.from() != self.next {
            return Err(errors::internal("Noncontiguous screen record"));
        }
        if self.recording {
            self.bridge_disk_gap(from, flush)?;
            self.disk_append(Record::Gap { from, to }, flush)?;
        }
        self.next = record.end();
        self.live.push_back(record);
        while self.live.len() > 1024 {
            if let Some(LiveRecord::Data { bytes, .. }) = self.live.pop_front() {
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
            // The disk reader returns a bounded batch. More persisted records
            // must be read on the next pump, not reported as missing output.
            if position < self.disk.next_offset() {
                return Ok(result);
            }
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
                LiveRecord::Data { offset, bytes } => {
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
                LiveRecord::Gap { from, to: end } => Record::Gap {
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

    #[test]
    fn replay_continues_after_a_bounded_disk_batch_without_inventing_a_gap() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        for offset in 0..300 {
            screen
                .append(Record::Data {
                    offset,
                    bytes: vec![b'x'],
                })
                .unwrap();
        }
        drop(screen);

        let mut screen = Screen::open(&path, true).unwrap();
        screen
            .append(Record::Data {
                offset: 300,
                bytes: vec![b'y'],
            })
            .unwrap();

        let mut next = 0;
        let mut batches = 0;
        let mut replayed = Vec::new();
        while next < 301 {
            let batch = screen
                .read(next, 301, limits::TERMINAL_PAYLOAD_BYTES)
                .unwrap();
            assert!(!batch.is_empty());
            batches += 1;
            for record in batch {
                let Record::Data { offset, bytes } = record else {
                    panic!("persisted data must not replay as a gap");
                };
                assert_eq!(offset, next);
                next += bytes.len() as u64;
                replayed.extend(bytes);
            }
        }
        assert!(batches > 1);
        assert_eq!(replayed, [vec![b'x'; 300], vec![b'y']].concat());
        fs::remove_dir_all(path).unwrap();
    }
}
