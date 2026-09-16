use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, HarnessIdentity, MAX_RETAINED_SEGMENTS,
    MAX_SEGMENT_BYTES, ObservationFrame, ObservationGap, RecordedRow, ReplayedSegment,
    SegmentMetadata, decode_segment, encode_row,
};
use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

const DIRECTORY_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
const SEGMENT_SUFFIX: &str = ".jsonl";
pub const OBSERVATION_QUEUE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOutcome {
    Queued,
    GapExtended,
}

struct ObservationQueue {
    capacity: usize,
    rows: VecDeque<ObservationFrame>,
    gap: Option<ObservationGap>,
    last_submitted_sequence: Option<u64>,
    closed: bool,
}

impl ObservationQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            rows: VecDeque::with_capacity(capacity),
            gap: None,
            last_submitted_sequence: None,
            closed: false,
        }
    }

    fn push(&mut self, frame: ObservationFrame) -> Result<QueueOutcome> {
        if self
            .last_submitted_sequence
            .is_some_and(|last| frame.sequence <= last)
        {
            return Err(soyeht_pane_state::Error::InvalidSequence.into());
        }
        self.last_submitted_sequence = Some(frame.sequence);
        if let Some(gap) = &mut self.gap {
            gap.last_timestamp_ms = frame.timestamp_ms;
            gap.last_sequence = frame.sequence;
            gap.dropped_observations = gap
                .dropped_observations
                .checked_add(1)
                .ok_or(soyeht_pane_state::Error::InvalidGap)?;
            return Ok(QueueOutcome::GapExtended);
        }
        if self.rows.len() == self.capacity {
            let first = self
                .rows
                .front()
                .ok_or_else(|| errors::internal("Full observation queue was empty"))?;
            let first_timestamp_ms = first.timestamp_ms;
            let first_sequence = first.sequence;
            let dropped_observations = u64::try_from(self.rows.len())
                .map_err(|_| errors::internal("Observation queue length is invalid"))?
                .checked_add(1)
                .ok_or(soyeht_pane_state::Error::InvalidGap)?;
            self.rows.clear();
            self.gap = Some(ObservationGap {
                pane_id: frame.pane_id,
                harness: frame.harness,
                first_timestamp_ms,
                last_timestamp_ms: frame.timestamp_ms,
                first_sequence,
                last_sequence: frame.sequence,
                dropped_observations,
            });
            return Ok(QueueOutcome::GapExtended);
        }
        self.rows.push_back(frame);
        Ok(QueueOutcome::Queued)
    }

    fn pop(&mut self) -> Option<RecordedRow> {
        self.rows
            .pop_front()
            .map(RecordedRow::Observation)
            .or_else(|| self.gap.take().map(RecordedRow::Gap))
    }
}

struct RecorderState {
    queue: ObservationQueue,
    failure: Option<String>,
}

type SharedRecorder = Arc<(Mutex<RecorderState>, Condvar)>;

pub struct ObservationRecorder {
    pane_id: String,
    harness: HarnessIdentity,
    shared: SharedRecorder,
    worker: Option<JoinHandle<Result<()>>>,
}

impl ObservationRecorder {
    pub fn open(store: SegmentStore) -> Result<Self> {
        Self::open_with_capacity(store, OBSERVATION_QUEUE_CAPACITY)
    }

    fn open_with_capacity(store: SegmentStore, capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(errors::invalid(
                "Observation queue capacity must be positive",
            ));
        }
        let pane_id = store.pane_id.clone();
        let harness = store.harness.clone();
        let shared = Arc::new((
            Mutex::new(RecorderState {
                queue: ObservationQueue::new(capacity),
                failure: None,
            }),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("pane-observation-writer".into())
            .spawn(move || recorder_loop(store, worker_shared))?;
        Ok(Self {
            pane_id,
            harness,
            shared,
            worker: Some(worker),
        })
    }

    pub fn try_record(&self, frame: ObservationFrame) -> Result<QueueOutcome> {
        frame.validate()?;
        if frame.pane_id != self.pane_id || frame.harness != self.harness {
            return Err(errors::invalid(
                "Observation frame provenance does not match its recorder",
            ));
        }
        let (lock, wake) = &*self.shared;
        let mut state = lock
            .lock()
            .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
        if let Some(failure) = &state.failure {
            return Err(errors::internal(failure.clone()));
        }
        if state.queue.closed {
            return Err(errors::internal("Observation recorder is closed"));
        }
        let outcome = state.queue.push(frame)?;
        wake.notify_one();
        Ok(outcome)
    }

    pub fn finish(mut self) -> Result<()> {
        self.stop_worker()
    }

    fn stop_worker(&mut self) -> Result<()> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let (lock, wake) = &*self.shared;
        lock.lock()
            .map_err(|_| errors::internal("Observation queue lock was poisoned"))?
            .queue
            .closed = true;
        wake.notify_one();
        worker
            .join()
            .map_err(|_| errors::internal("Observation writer thread panicked"))?
    }
}

impl Drop for ObservationRecorder {
    fn drop(&mut self) {
        let _ = self.stop_worker();
    }
}

fn recorder_loop(mut store: SegmentStore, shared: SharedRecorder) -> Result<()> {
    loop {
        let row = next_recorded_row(&shared)?;
        let Some(row) = row else { return Ok(()) };
        if let Err(error) = store.append(row) {
            let (lock, wake) = &*shared;
            if let Ok(mut state) = lock.lock() {
                state.failure = Some(error.to_string());
                state.queue.closed = true;
                wake.notify_all();
            }
            return Err(error);
        }
    }
}

fn next_recorded_row(shared: &SharedRecorder) -> Result<Option<RecordedRow>> {
    let (lock, wake) = &**shared;
    let mut state = lock
        .lock()
        .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
    loop {
        if let Some(row) = state.queue.pop() {
            return Ok(Some(row));
        }
        if state.queue.closed {
            return Ok(None);
        }
        state = wake
            .wait(state)
            .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletionReceipt {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug)]
struct Segment {
    number: u64,
    size: usize,
    metadata: SegmentMetadata,
}

pub struct SegmentStore {
    root: PathBuf,
    directory: PathBuf,
    pane_id: String,
    harness: HarnessIdentity,
    engine: EngineBuild,
    segments: VecDeque<Segment>,
    last_sequence: Option<u64>,
}

impl SegmentStore {
    pub fn open(
        root: &Path,
        pane_id: impl Into<String>,
        harness: HarnessIdentity,
        engine: EngineBuild,
    ) -> Result<Self> {
        let pane_id = pane_id.into();
        validate_identity(&pane_id, &harness, &engine)?;
        ensure_private_directory(root)?;
        let directory = root.join(pane_directory_name(&pane_id));
        ensure_private_directory(&directory)?;
        let (segments, last_sequence) = load_segments(&directory, &pane_id)?;
        Ok(Self {
            root: root.into(),
            directory,
            pane_id,
            harness,
            engine,
            segments,
            last_sequence,
        })
    }

    pub fn append(&mut self, row: RecordedRow) -> Result<()> {
        row.validate()?;
        let (pane_id, harness, timestamp_ms, sequence) = row_identity(&row);
        if pane_id != self.pane_id || harness != &self.harness {
            return Err(errors::invalid(
                "Observation row provenance does not match its pane",
            ));
        }
        if self.last_sequence.is_some_and(|last| sequence <= last) {
            return Err(soyeht_pane_state::Error::InvalidSequence.into());
        }
        let encoded = encode_recorded_row(&row)?;
        if self.needs_rotation(encoded.len()) {
            self.create_segment(timestamp_ms)?;
        }
        self.append_bytes(&encoded)?;
        self.last_sequence = Some(sequence);
        self.enforce_retention()
    }

    pub fn replay(&self) -> Result<Vec<ReplayedSegment>> {
        self.segments
            .iter()
            .map(|segment| read_segment(&self.directory, segment.number))
            .collect()
    }

    pub fn delete(self) -> Result<DeletionReceipt> {
        let root = self.root.clone();
        let pane_id = self.pane_id.clone();
        drop(self);
        delete_pane(&root, &pane_id)
    }

    fn needs_rotation(&self, row_bytes: usize) -> bool {
        self.segments.back().is_none_or(|segment| {
            segment.metadata.harness != self.harness
                || segment.metadata.engine != self.engine
                || segment.size.saturating_add(row_bytes) > MAX_SEGMENT_BYTES
        })
    }

    fn create_segment(&mut self, started_at_ms: i64) -> Result<()> {
        let number = self
            .segments
            .back()
            .map(|segment| {
                segment
                    .number
                    .checked_add(1)
                    .ok_or_else(|| errors::internal("Observation segment sequence exhausted"))
            })
            .transpose()?
            .unwrap_or(0);
        let metadata = SegmentMetadata {
            schema_version: DATASET_SCHEMA_VERSION,
            pane_id: self.pane_id.clone(),
            harness: self.harness.clone(),
            engine: self.engine.clone(),
            segment_sequence: number,
            started_at_ms,
        };
        let header = encode_row(&DatasetRow::Metadata(metadata.clone()))?;
        if header.len() >= MAX_SEGMENT_BYTES {
            return Err(errors::internal(
                "Observation segment header exceeds its bound",
            ));
        }
        create_segment_file(&self.directory, number, &header)?;
        self.segments.push_back(Segment {
            number,
            size: header.len(),
            metadata,
        });
        Ok(())
    }

    fn append_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let segment = self
            .segments
            .back_mut()
            .ok_or_else(|| errors::internal("Observation segment is missing"))?;
        let path = self.directory.join(segment_filename(segment.number));
        let mut file = OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        validate_private_file(&file)?;
        let actual = usize::try_from(file.metadata()?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        if actual != segment.size || actual.saturating_add(bytes.len()) > MAX_SEGMENT_BYTES {
            return Err(errors::internal(
                "Observation segment changed outside the recorder",
            ));
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        segment.size = segment.size.saturating_add(bytes.len());
        Ok(())
    }

    fn enforce_retention(&mut self) -> Result<()> {
        while self.segments.len() > MAX_RETAINED_SEGMENTS {
            let oldest = self
                .segments
                .pop_front()
                .ok_or_else(|| errors::internal("Observation retention lost its segment"))?;
            fs::remove_file(self.directory.join(segment_filename(oldest.number)))?;
            File::open(&self.directory)?.sync_all()?;
        }
        Ok(())
    }
}

pub fn delete_pane(root: &Path, pane_id: &str) -> Result<DeletionReceipt> {
    if pane_id.is_empty() {
        return Err(errors::invalid(
            "Pane identity is required for observation deletion",
        ));
    }
    validate_private_directory(root)?;
    let directory = root.join(pane_directory_name(pane_id));
    match fs::symlink_metadata(&directory) {
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DeletionReceipt { files: 0, bytes: 0 });
        }
        Err(error) => return Err(error.into()),
    }
    validate_private_directory(&directory)?;
    let files = validated_segment_files(&directory)?;
    let receipt = deletion_receipt(&files)?;
    for path in files {
        fs::remove_file(path)?;
    }
    File::open(&directory)?.sync_all()?;
    fs::remove_dir(&directory)?;
    File::open(root)?.sync_all()?;
    Ok(receipt)
}

fn validate_identity(pane_id: &str, harness: &HarnessIdentity, engine: &EngineBuild) -> Result<()> {
    let metadata = SegmentMetadata {
        schema_version: DATASET_SCHEMA_VERSION,
        pane_id: pane_id.into(),
        harness: harness.clone(),
        engine: engine.clone(),
        segment_sequence: 0,
        started_at_ms: 0,
    };
    Ok(metadata.validate()?)
}

fn load_segments(directory: &Path, pane_id: &str) -> Result<(VecDeque<Segment>, Option<u64>)> {
    let mut numbers = segment_numbers(directory)?;
    numbers.sort_unstable();
    let mut segments = VecDeque::new();
    let mut last_sequence = None;
    for number in numbers {
        let replayed = read_segment(directory, number)?;
        validate_loaded_segment(&replayed, pane_id, number, &mut last_sequence)?;
        let size = usize::try_from(fs::metadata(directory.join(segment_filename(number)))?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        segments.push_back(Segment {
            number,
            size,
            metadata: replayed.metadata,
        });
    }
    Ok((segments, last_sequence))
}

fn validate_loaded_segment(
    segment: &ReplayedSegment,
    pane_id: &str,
    number: u64,
    last_sequence: &mut Option<u64>,
) -> Result<()> {
    if segment.metadata.pane_id != pane_id || segment.metadata.segment_sequence != number {
        return Err(soyeht_pane_state::Error::MismatchedProvenance.into());
    }
    for row in &segment.rows {
        let sequence = row_identity(row).3;
        if last_sequence.is_some_and(|last| sequence <= last) {
            return Err(soyeht_pane_state::Error::InvalidSequence.into());
        }
        *last_sequence = Some(sequence);
    }
    Ok(())
}

fn read_segment(directory: &Path, number: u64) -> Result<ReplayedSegment> {
    let path = directory.join(segment_filename(number));
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    validate_private_file(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SEGMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(decode_segment(&bytes)?)
}

fn segment_numbers(directory: &Path) -> Result<Vec<u64>> {
    let mut numbers = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() || !entry.file_type()?.is_file() {
            return Err(errors::internal(
                "Observation directory contains an invalid entry",
            ));
        }
        let name = entry.file_name();
        let number = parse_segment_name(&name)
            .ok_or_else(|| errors::internal("Observation segment name is invalid"))?;
        numbers.push(number);
    }
    Ok(numbers)
}

fn validated_segment_files(directory: &Path) -> Result<Vec<PathBuf>> {
    segment_numbers(directory).map(|numbers| {
        numbers
            .into_iter()
            .map(|number| directory.join(segment_filename(number)))
            .collect()
    })
}

fn deletion_receipt(paths: &[PathBuf]) -> Result<DeletionReceipt> {
    let mut bytes = 0_u64;
    for path in paths {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(errors::internal(
                "Observation deletion refuses a non-file entry",
            ));
        }
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| errors::internal("Observation deletion size overflow"))?;
    }
    Ok(DeletionReceipt {
        files: paths.len(),
        bytes,
    })
}

fn create_segment_file(directory: &Path, number: u64, header: &[u8]) -> Result<()> {
    let path = directory.join(segment_filename(number));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(header)?;
    file.sync_all()?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_private_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(DIRECTORY_MODE).create(path)?;
            validate_private_directory(path)
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(errors::internal(
            "Observation directory is not private and symlink-free",
        ));
    }
    Ok(())
}

fn validate_private_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(errors::internal(
            "Observation segment is not a private regular file",
        ));
    }
    Ok(())
}

fn pane_directory_name(pane_id: &str) -> String {
    format!("pane-{:x}", Sha256::digest(pane_id.as_bytes()))
}

fn segment_filename(number: u64) -> String {
    format!("{number:016x}{SEGMENT_SUFFIX}")
}

fn parse_segment_name(name: &OsStr) -> Option<u64> {
    let text = name.to_str()?;
    let stem = text.strip_suffix(SEGMENT_SUFFIX)?;
    (stem.len() == 16)
        .then(|| u64::from_str_radix(stem, 16).ok())
        .flatten()
}

fn row_identity(row: &RecordedRow) -> (&str, &HarnessIdentity, i64, u64) {
    match row {
        RecordedRow::Observation(frame) => (
            &frame.pane_id,
            &frame.harness,
            frame.timestamp_ms,
            frame.sequence,
        ),
        RecordedRow::Label(label) => (
            &label.pane_id,
            &label.harness,
            label.timestamp_ms,
            label.sequence,
        ),
        RecordedRow::Gap(gap) => (
            &gap.pane_id,
            &gap.harness,
            gap.last_timestamp_ms,
            gap.last_sequence,
        ),
    }
}

fn encode_recorded_row(row: &RecordedRow) -> Result<Vec<u8>> {
    match row {
        RecordedRow::Observation(frame) => Ok(encode_row(&DatasetRow::Observation(frame.clone()))?),
        RecordedRow::Label(label) => Ok(encode_row(&DatasetRow::Label(label.clone()))?),
        RecordedRow::Gap(gap) => Ok(encode_row(&DatasetRow::Gap(gap.clone()))?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_pane_state::{MAX_SCREEN_LINE_BYTES, MAX_SCREEN_LINES, ObservationFrame};
    use std::os::unix::fs::symlink;
    use uuid::Uuid;

    struct Fixture {
        directory: PathBuf,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("sn-observations-{}", Uuid::new_v4()));
            fs::DirBuilder::new()
                .mode(DIRECTORY_MODE)
                .create(&directory)
                .unwrap();
            let root = directory.join("raw-terminal");
            Self { directory, root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn harness() -> HarnessIdentity {
        HarnessIdentity::known("registry.test", Some("1.2.3".into())).unwrap()
    }

    fn engine(digest: char) -> EngineBuild {
        EngineBuild {
            package_version: "0.1.0".into(),
            executable_sha256: digest.to_string().repeat(64),
        }
    }

    fn observation(sequence: u64, large: bool) -> RecordedRow {
        let lines = if large {
            vec!["x".repeat(MAX_SCREEN_LINE_BYTES); MAX_SCREEN_LINES]
        } else {
            vec![format!("frame {sequence}")]
        };
        RecordedRow::Observation(ObservationFrame {
            pane_id: "pane-1".into(),
            harness: harness(),
            timestamp_ms: 1_800_000_000_000 + sequence as i64,
            sequence,
            osc_title: Some("terminal".into()),
            lines,
            cursor: None,
            foreground_process: Some("test".into()),
            bytes_per_second: 42,
        })
    }

    fn frame(sequence: u64) -> ObservationFrame {
        let RecordedRow::Observation(frame) = observation(sequence, false) else {
            unreachable!()
        };
        frame
    }

    #[test]
    fn bounded_queue_makes_sustained_pressure_the_next_durable_row() {
        let mut queue = ObservationQueue::new(2);
        assert_eq!(queue.push(frame(0)).unwrap(), QueueOutcome::Queued);
        assert_eq!(queue.push(frame(1)).unwrap(), QueueOutcome::Queued);
        assert_eq!(queue.push(frame(2)).unwrap(), QueueOutcome::GapExtended);
        let mut rolled_back = frame(3);
        rolled_back.timestamp_ms -= 100;
        assert_eq!(queue.push(rolled_back).unwrap(), QueueOutcome::GapExtended);
        let Some(RecordedRow::Gap(gap)) = queue.pop() else {
            panic!("queue pressure did not leave a gap")
        };
        assert_eq!(gap.first_sequence, 0);
        assert_eq!(gap.last_sequence, 3);
        assert_eq!(gap.dropped_observations, 4);
        assert!(gap.last_timestamp_ms < gap.first_timestamp_ms);
        gap.validate().unwrap();
        assert_eq!(queue.push(frame(4)).unwrap(), QueueOutcome::Queued);
        assert_eq!(queue.pop(), Some(observation(4, false)));
    }

    #[test]
    fn asynchronous_writer_drains_without_losing_a_frame() {
        let fixture = Fixture::new();
        let store = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        let recorder = ObservationRecorder::open(store).unwrap();
        for sequence in 0..8 {
            assert_eq!(
                recorder.try_record(frame(sequence)).unwrap(),
                QueueOutcome::Queued
            );
        }
        recorder.finish().unwrap();
        let replayed = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a'))
            .unwrap()
            .replay()
            .unwrap();
        assert_eq!(
            replayed
                .iter()
                .flat_map(|segment| segment.rows.iter())
                .cloned()
                .collect::<Vec<_>>(),
            (0..8)
                .map(|sequence| observation(sequence, false))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rotation_replays_every_whole_row_and_repeats_exact_provenance() {
        let fixture = Fixture::new();
        let mut store =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        for sequence in 0..9 {
            store.append(observation(sequence, true)).unwrap();
        }
        let replayed = store.replay().unwrap();
        assert!(replayed.len() > 1);
        assert_eq!(
            replayed
                .iter()
                .map(|segment| segment.rows.len())
                .sum::<usize>(),
            9
        );
        for segment in replayed {
            assert_eq!(segment.metadata.pane_id, "pane-1");
            assert_eq!(segment.metadata.harness, harness());
            assert_eq!(segment.metadata.engine, engine('a'));
        }
    }

    #[test]
    fn restart_keeps_sequence_and_build_change_starts_a_self_describing_segment() {
        let fixture = Fixture::new();
        let mut first =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        first.append(observation(0, false)).unwrap();
        drop(first);
        let mut second =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('b')).unwrap();
        second.append(observation(1, false)).unwrap();
        let replayed = second.replay().unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].metadata.engine, engine('a'));
        assert_eq!(replayed[1].metadata.engine, engine('b'));
        assert_eq!(replayed[0].rows, vec![observation(0, false)]);
        assert_eq!(replayed[1].rows, vec![observation(1, false)]);
    }

    #[test]
    fn retention_removes_only_whole_old_segments() {
        let fixture = Fixture::new();
        let mut store =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        for sequence in 0..70 {
            store.append(observation(sequence, true)).unwrap();
        }
        let replayed = store.replay().unwrap();
        assert_eq!(replayed.len(), MAX_RETAINED_SEGMENTS);
        let rows = replayed
            .iter()
            .flat_map(|segment| segment.rows.iter())
            .collect::<Vec<_>>();
        assert!(rows.len() < 70);
        assert_eq!(rows.last(), Some(&&observation(69, true)));
    }

    #[test]
    fn private_permissions_and_delete_refuse_symlinks_before_removing_anything() {
        let fixture = Fixture::new();
        let mut store =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        store.append(observation(0, false)).unwrap();
        let pane_directory = fixture.root.join(pane_directory_name("pane-1"));
        assert_eq!(
            fs::metadata(&fixture.root).unwrap().permissions().mode() & 0o777,
            DIRECTORY_MODE
        );
        let segment = validated_segment_files(&pane_directory).unwrap().remove(0);
        assert_eq!(
            fs::metadata(&segment).unwrap().permissions().mode() & 0o777,
            FILE_MODE
        );
        let outside = fixture.directory.join("outside");
        fs::write(&outside, "private").unwrap();
        symlink(&outside, pane_directory.join("0000000000000001.jsonl")).unwrap();
        assert!(delete_pane(&fixture.root, "pane-1").is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
        assert!(segment.exists());
        fs::remove_file(pane_directory.join("0000000000000001.jsonl")).unwrap();
        let receipt = store.delete().unwrap();
        assert_eq!(receipt.files, 1);
        assert!(receipt.bytes > 0);
        assert!(!pane_directory.exists());
    }

    #[test]
    fn partial_tail_and_dangling_pane_symlink_fail_closed() {
        let fixture = Fixture::new();
        let mut store =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        store.append(observation(0, false)).unwrap();
        let pane_directory = fixture.root.join(pane_directory_name("pane-1"));
        let segment = validated_segment_files(&pane_directory).unwrap().remove(0);
        let length = fs::metadata(&segment).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&segment)
            .unwrap()
            .set_len(length - 1)
            .unwrap();
        drop(store);
        assert!(SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).is_err());

        let dangling = fixture.root.join(pane_directory_name("pane-2"));
        symlink(fixture.directory.join("missing"), &dangling).unwrap();
        assert!(delete_pane(&fixture.root, "pane-2").is_err());
        assert!(
            fs::symlink_metadata(dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
