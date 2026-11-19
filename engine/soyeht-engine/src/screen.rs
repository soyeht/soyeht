use crate::errors::{self, Result};
use soyeht_protocol::limits;
use soyeht_store::segments::{Log, Reader, Record, Unsynced};
use std::{collections::VecDeque, path::Path, sync::Arc};

/// A record kept in memory: the output bytes are shared with the supervisor, so a clone is cheap.
type LiveRecord = Record<Arc<Vec<u8>>>;

/// The records a screen keeps in memory at most, so a terminal of tiny records holds bounded memory.
const LIVE_RECORDS: usize = 1024;

/// The most a record in memory of a pane that does not record grows to by taking in the short
/// records that follow it. Two neighbours that are not merged hold more than this together, so
/// `LIVE_RECORDS` records hold more than `TAIL_BYTES` whatever size the pty hands its output over
/// in, and the count never cuts a tail short. It is far below `TERMINAL_PAYLOAD_BYTES`, so a
/// record is still one frame at most.
const MERGED_RECORD_BYTES: usize = limits::TAIL_BYTES / (LIVE_RECORDS / 2);

pub struct Screen {
    /// The pane's output on disk. `None` once the pane is closed (see [`Screen::release`]).
    disk: Option<Log>,
    live: VecDeque<LiveRecord>,
    live_bytes: usize,
    next: u64,
    /// The supervisor's output position this screen has taken in and not yet told it to drop.
    unacknowledged: Option<u64>,
    pub recording: bool,
}

/// What one slice of a replay is read from: the memory of the screen, read already, or the
/// disk, read by whoever holds the [`Reader`] with no lock.
pub enum ReplayRead {
    Ready(Vec<Record>),
    Disk(Reader),
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

fn closed_screen() -> errors::Error {
    errors::internal("A closed pane keeps no output")
}

impl Screen {
    pub fn open(directory: &Path, recording: bool) -> Result<Self> {
        let disk = Log::open(directory)?;
        Ok(Self {
            next: disk.next_offset(),
            disk: Some(disk),
            live: VecDeque::new(),
            live_bytes: 0,
            unacknowledged: None,
            recording,
        })
    }

    /// The screen of a pane that is already closed: it holds no output and never takes any.
    pub fn closed() -> Self {
        Self {
            disk: None,
            live: VecDeque::new(),
            live_bytes: 0,
            next: 0,
            unacknowledged: None,
            recording: false,
        }
    }

    /// Lets go of everything the screen holds in memory (the tail and the index of the segment
    /// files) once its pane is closed. Nothing reads the output of a closed pane again, and
    /// the segment files stay on the disk.
    pub fn release(&mut self) {
        self.disk = None;
        self.live = VecDeque::new();
        self.live_bytes = 0;
        self.unacknowledged = None;
    }

    /// The bytes of output held in memory.
    #[cfg(test)]
    pub(crate) fn held_bytes(&self) -> usize {
        self.live_bytes
    }

    pub fn next_offset(&self) -> u64 {
        self.next
    }
    pub fn first_offset(&self) -> u64 {
        let on_disk = self.disk.as_ref().map_or(self.next, Log::first_offset);
        on_disk.min(self.live.front().map(Record::from).unwrap_or(self.next))
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
            unsynced: self.disk.as_mut().and_then(Log::take_unsynced),
            through,
        })
    }

    /// Owes `durable` again, when its flush failed: the next one retries it.
    pub fn restore_durable(&mut self, durable: &Durable) {
        self.take_in_through(durable.through);
        if let (Some(unsynced), Some(disk)) = (&durable.unsynced, &mut self.disk) {
            disk.restore_unsynced(unsynced);
        }
    }

    fn append_data_record(&mut self, offset: u64, bytes: Arc<Vec<u8>>, flush: bool) -> Result<()> {
        self.require_open()?;
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
        self.keep_live(record);
        Ok(())
    }

    /// Keeps `record` in memory, after what is there, and lets go of the oldest output past the
    /// bounds. Where nothing is recorded, memory is the whole tail, and a short data record joins
    /// the one before it: a terminal that reads in small pieces would otherwise run out of records
    /// long before it holds a tail. Where the disk records the output, the disk serves the tail and
    /// every record stays as it came, which is the unit an agent's output is decoded in.
    fn keep_live(&mut self, record: LiveRecord) {
        let merged = match (&record, self.live.back_mut()) {
            (LiveRecord::Data { bytes, .. }, Some(LiveRecord::Data { bytes: last, .. }))
                if !self.recording && last.len() + bytes.len() <= MERGED_RECORD_BYTES =>
            {
                let last = Arc::make_mut(last);
                last.reserve_exact(bytes.len());
                last.extend_from_slice(bytes);
                true
            }
            _ => false,
        };
        if let LiveRecord::Data { bytes, .. } = &record {
            self.live_bytes += bytes.len();
        }
        if !merged {
            self.live.push_back(record);
        }
        while self.live_bytes > limits::TAIL_BYTES || self.live.len() > LIVE_RECORDS {
            if let Some(LiveRecord::Data { bytes, .. }) = self.live.pop_front() {
                self.live_bytes -= bytes.len();
            }
        }
    }

    /// Records the stretch the disk missed (while recording was off) before a record at `to`.
    fn bridge_disk_gap(&mut self, to: u64, flush: bool) -> Result<()> {
        let from = self.disk()?.next_offset();
        if from < to {
            self.disk_append(Record::Gap { from, to }, flush)?;
        }
        Ok(())
    }

    fn disk_append(&mut self, record: Record, flush: bool) -> Result<()> {
        let disk = self.disk_mut()?;
        Ok(if flush {
            disk.append(record)
        } else {
            disk.append_unsynced(record)
        }?)
    }

    fn disk(&self) -> Result<&Log> {
        self.disk.as_ref().ok_or_else(closed_screen)
    }

    fn disk_mut(&mut self) -> Result<&mut Log> {
        self.disk.as_mut().ok_or_else(closed_screen)
    }

    fn require_open(&self) -> Result<()> {
        self.disk().map(drop)
    }

    fn append_gap(&mut self, from: u64, to: u64, flush: bool) -> Result<()> {
        self.require_open()?;
        let record = LiveRecord::Gap { from, to };
        if record.from() != self.next {
            return Err(errors::internal("Noncontiguous screen record"));
        }
        if self.recording {
            self.bridge_disk_gap(from, flush)?;
            self.disk_append(Record::Gap { from, to }, flush)?;
        }
        self.next = record.end();
        self.keep_live(record);
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
            // The disk serves what memory no longer holds only when it reaches memory: with a
            // hole between them (recording was off), the tail starts at memory, where the
            // output is whole, and no gap is announced for bytes that are still held.
            match self.disk.as_ref() {
                Some(disk) if disk.next_offset() >= live_first => {
                    disk.align_forward(desired).min(live_first)
                }
                _ => live_first,
            }
        } else {
            self.live
                .iter()
                .find(|record| record.from() >= desired)
                .map(Record::from)
                .unwrap_or(self.next)
        }
    }

    /// The next slice of at most `maximum` bytes of a replay from `from` up to `to`. Output that
    /// is only on the disk is not read here, which would wait for the disk under the Core lock.
    pub fn replay_read(&self, from: u64, to: u64, maximum: usize) -> Result<ReplayRead> {
        let live_first = self.live.front().map(Record::from).unwrap_or(self.next);
        let on_disk = self.disk.as_ref().map_or(0, Log::next_offset);
        match self
            .disk
            .as_ref()
            .filter(|_| from < live_first && from < on_disk)
        {
            Some(disk) => disk
                .reader(from, to.min(live_first).min(on_disk), maximum)
                .map(ReplayRead::Disk)
                .map_err(errors::storage),
            None => self.read(from, to, maximum).map(ReplayRead::Ready),
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
        let on_disk = self.disk.as_ref().map_or(0, Log::next_offset);
        if let Some(disk) = self
            .disk
            .as_ref()
            .filter(|_| position < live_first && position < on_disk)
        {
            for record in disk.read(position, to.min(live_first).min(on_disk), remaining)? {
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
            if position < on_disk {
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
    fn a_released_screen_holds_no_output_and_takes_none() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        screen.append_data(0, vec![b'x'; 1000]).unwrap();
        assert_eq!(screen.held_bytes(), 1000);

        screen.release();

        assert_eq!(screen.held_bytes(), 0);
        assert_eq!(screen.next_offset(), 1000);
        assert_eq!(screen.first_offset(), 1000);
        assert_eq!(screen.tail_start(), 1000);
        assert!(screen.read(1000, 1000, 64).unwrap().is_empty());
        assert!(screen.take_durable().is_none());
        assert!(screen.append_data(1000, vec![b'y']).is_err());
        // The segment files are the pane's record and stay.
        assert!(fs::read_dir(&path).unwrap().next().is_some());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn the_screen_of_a_pane_that_was_closed_before_the_engine_started_is_empty() {
        let mut screen = Screen::closed();
        assert_eq!((screen.first_offset(), screen.next_offset()), (0, 0));
        assert_eq!(screen.held_bytes(), 0);
        assert!(screen.append_data(0, vec![b'x']).is_err());
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

    /// The bytes of `[from, to)` the screen replays, which must all be data.
    fn replayed(screen: &Screen, from: u64, to: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut next = from;
        while next < to {
            let batch = screen
                .read(next, to, limits::TERMINAL_PAYLOAD_BYTES)
                .unwrap();
            assert!(!batch.is_empty(), "no progress at {next}");
            for record in batch {
                let Record::Data {
                    offset,
                    bytes: data,
                } = record
                else {
                    panic!("a gap at {next} for output the screen holds");
                };
                assert_eq!(offset, next);
                next += data.len() as u64;
                bytes.extend(data);
            }
        }
        bytes
    }

    /// The pattern the short writes carry, so a replay is checked byte for byte.
    fn pattern(offset: u64) -> u8 {
        (offset / 7 % 251) as u8
    }

    const SHORT_WRITE: usize = 128;

    fn feed_short_writes(screen: &mut Screen, from: u64, to: u64) {
        let mut offset = from;
        while offset < to {
            let end = (offset + SHORT_WRITE as u64).min(to);
            screen
                .append_deferred(Record::Data {
                    offset,
                    bytes: (offset..end).map(pattern).collect(),
                })
                .unwrap();
            offset = end;
        }
    }

    /// The tail the screen replays for output that ended at `many`: it starts within the contract's
    /// window (section 6 of contract 08), and is contiguous, with no gap, and byte for byte what was
    /// written. Returns where it starts.
    fn assert_whole_tail(screen: &Screen, many: u64) -> u64 {
        let start = screen.tail_start();
        assert!(
            many - start >= (limits::TAIL_BYTES - limits::TERMINAL_PAYLOAD_BYTES) as u64
                && many - start <= limits::TAIL_BYTES as u64,
            "the tail starts at {start} of {many}"
        );
        let expected: Vec<u8> = (start..many).map(pattern).collect();
        assert_eq!(replayed(screen, start, many), expected);
        start
    }

    /// A terminal that does not record to the disk keeps a tail in memory only. The pty hands its
    /// output over in short reads, and the tail must still be what the contract promises: the last
    /// `TAIL_BYTES`, to the record boundary after them (the window of contract 08 section 6), as one
    /// contiguous replay with no gap, while memory stays bounded.
    #[test]
    fn short_writes_without_recording_replay_the_tail_the_contract_promises() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, false).unwrap();
        let few = 64 * 1024;
        feed_short_writes(&mut screen, 0, few);
        // Less than a tail was written: all of it replays.
        assert_eq!(screen.tail_start(), 0);
        let expected: Vec<u8> = (0..few).map(pattern).collect();
        assert_eq!(replayed(&screen, 0, few), expected);

        let many = (limits::TAIL_BYTES * 3) as u64;
        feed_short_writes(&mut screen, few, many);
        assert_whole_tail(&screen, many);
        // Memory stays bounded by the tail, in bytes and in records.
        assert!(screen.held_bytes() <= limits::TAIL_BYTES);
        assert!(screen.live.len() <= LIVE_RECORDS);
        fs::remove_dir_all(path).unwrap();
    }

    /// A recorded terminal keeps each record it is given, on the disk and in memory, and its replay
    /// reads the tail from the disk past what memory holds.
    #[test]
    fn short_writes_with_recording_keep_their_records() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        let many = (limits::TAIL_BYTES * 3) as u64;
        feed_short_writes(&mut screen, 0, many);
        assert_whole_tail(&screen, many);
        // Memory keeps the records as they came too: they are the unit an agent's output decodes in.
        assert!(
            screen
                .live
                .iter()
                .all(|record| record.end() - record.from() == SHORT_WRITE as u64)
        );
        drop(screen);

        let screen = Screen::open(&path, true).unwrap();
        let records = screen.read(0, many, SHORT_WRITE * 4).unwrap();
        assert!(
            records
                .iter()
                .all(|record| record.end() - record.from() == SHORT_WRITE as u64)
        );
        fs::remove_dir_all(path).unwrap();
    }

    /// A hole the disk does not fill (recording was switched off) is not part of the tail: the tail
    /// starts where memory holds the output whole, and announces no gap for it.
    #[test]
    fn a_tail_never_starts_before_a_hole_the_disk_does_not_fill() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        feed_short_writes(&mut screen, 0, 1024);
        screen.recording = false;
        let many = (limits::TAIL_BYTES * 3) as u64;
        feed_short_writes(&mut screen, 1024, many);
        assert!(assert_whole_tail(&screen, many) > 1024);
        fs::remove_dir_all(path).unwrap();
    }

    /// Recording switched off and on again, and output a capture shares with the screen: the bytes
    /// a replay reads are the bytes that were written, and what the capture holds does not change.
    #[test]
    fn switching_recording_and_sharing_bytes_do_not_change_what_a_replay_reads() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, true).unwrap();
        let third = limits::TAIL_BYTES as u64;
        feed_short_writes(&mut screen, 0, third);
        screen.recording = false;
        feed_short_writes(&mut screen, third, 2 * third);
        let shared = Arc::new(
            (2 * third..2 * third + 100)
                .map(pattern)
                .collect::<Vec<u8>>(),
        );
        screen
            .append_shared_data_deferred(2 * third, Arc::clone(&shared))
            .unwrap();
        feed_short_writes(&mut screen, 2 * third + 100, 2 * third + 200);
        screen.recording = true;
        let many = 3 * third;
        feed_short_writes(&mut screen, 2 * third + 200, many);
        assert_eq!(shared.len(), 100, "joining records changed a shared one");
        assert_whole_tail(&screen, many);
        fs::remove_dir_all(path).unwrap();
    }

    /// A record that is a gap is never joined to the data around it.
    #[test]
    fn a_gap_between_short_records_stays_a_gap() {
        let path = std::env::temp_dir().join(format!("soyeht-next-screen-{}", Uuid::new_v4()));
        let mut screen = Screen::open(&path, false).unwrap();
        feed_short_writes(&mut screen, 0, 300);
        screen
            .append_deferred(Record::Gap { from: 300, to: 500 })
            .unwrap();
        feed_short_writes(&mut screen, 500, 800);
        let records = screen.read(0, 800, limits::TERMINAL_PAYLOAD_BYTES).unwrap();
        let shape: Vec<(bool, u64, u64)> = records
            .iter()
            .map(|record| {
                (
                    matches!(record, Record::Gap { .. }),
                    record.from(),
                    record.end(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [(false, 0, 300), (true, 300, 500), (false, 500, 800)]
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
