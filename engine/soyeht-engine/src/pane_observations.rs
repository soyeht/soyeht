use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, GroundTruthLabel, HarnessIdentity,
    MAX_RETAINED_SEGMENTS, MAX_SEGMENT_BYTES, ObservationFrame, ObservationGap, RecordedRow,
    ReplayedSegment, SegmentEnd, SegmentMetadata, decode_segment, encode_row,
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
    time::{Duration, Instant},
};

const DIRECTORY_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
const SEGMENT_SUFFIX: &str = ".jsonl";
pub const OBSERVATION_QUEUE_CAPACITY: usize = 32;
const OBSERVATION_WRITE_BATCH: usize = 8;
const OBSERVATION_WRITE_MAX_WAIT: Duration = Duration::from_millis(50);
const LABEL_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const LABEL_WRITE_TIMEOUT_MESSAGE: &str =
    "The observation writer stayed too slow to accept a ground-truth label for 30 seconds";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOutcome {
    Queued,
    GapExtended,
}

struct ObservationQueue {
    capacity: usize,
    rows: VecDeque<RecordedRow>,
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
        self.push_row(RecordedRow::Observation(frame))
    }

    fn push_gap(&mut self, gap: ObservationGap) -> Result<QueueOutcome> {
        self.push_row(RecordedRow::Gap(gap))
    }

    fn push_row(&mut self, row: RecordedRow) -> Result<QueueOutcome> {
        let (_, _, _, sequence) = row_identity(&row);
        if self
            .last_submitted_sequence
            .is_some_and(|last| sequence <= last)
        {
            return Err(soyeht_pane_state::Error::InvalidSequence.into());
        }
        self.last_submitted_sequence = Some(sequence);
        if let Some(gap) = &mut self.gap {
            extend_gap(gap, &row)?;
            return Ok(QueueOutcome::GapExtended);
        }
        if self.rows.len() == self.capacity {
            let first = self
                .rows
                .front()
                .ok_or_else(|| errors::internal("Full observation queue was empty"))?;
            let (pane_id, harness, first_timestamp_ms, first_sequence) = row_start(first);
            let pane_id = pane_id.to_owned();
            let harness = harness.clone();
            let mut gap = ObservationGap {
                pane_id,
                harness,
                first_timestamp_ms,
                last_timestamp_ms: first_timestamp_ms,
                first_sequence,
                last_sequence: first_sequence,
                dropped_observations: None,
                dropped_bytes: None,
            };
            let rows = self.rows.drain(..).chain(std::iter::once(row));
            for lost in rows {
                extend_gap(&mut gap, &lost)?;
            }
            self.rows.clear();
            gap.validate()?;
            self.gap = Some(gap);
            return Ok(QueueOutcome::GapExtended);
        }
        self.rows.push_back(row);
        Ok(QueueOutcome::Queued)
    }

    fn pop(&mut self) -> Option<RecordedRow> {
        self.rows
            .pop_front()
            .or_else(|| self.gap.take().map(RecordedRow::Gap))
    }

    fn pending(&self) -> usize {
        self.rows.len() + usize::from(self.gap.is_some())
    }

    fn drain_batch(&mut self) -> Vec<RecordedRow> {
        let count = self.pending().min(OBSERVATION_WRITE_BATCH);
        (0..count).filter_map(|_| self.pop()).collect()
    }
}

fn row_start(row: &RecordedRow) -> (&str, &HarnessIdentity, i64, u64) {
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
            gap.first_timestamp_ms,
            gap.first_sequence,
        ),
    }
}

fn add_loss(total: &mut Option<u64>, count: Option<u64>) -> Result<()> {
    if let Some(count) = count {
        *total = Some(
            total
                .unwrap_or(0)
                .checked_add(count)
                .ok_or(soyeht_pane_state::Error::InvalidGap)?,
        );
    }
    Ok(())
}

fn extend_gap(gap: &mut ObservationGap, row: &RecordedRow) -> Result<()> {
    let (pane_id, harness, timestamp_ms, sequence) = row_identity(row);
    if pane_id != gap.pane_id || harness != &gap.harness {
        return Err(errors::invalid("Observation gap provenance changed"));
    }
    gap.last_timestamp_ms = timestamp_ms;
    gap.last_sequence = sequence;
    match row {
        RecordedRow::Observation(_) => add_loss(&mut gap.dropped_observations, Some(1))?,
        RecordedRow::Gap(next) => {
            add_loss(&mut gap.dropped_observations, next.dropped_observations)?;
            add_loss(&mut gap.dropped_bytes, next.dropped_bytes)?;
        }
        RecordedRow::Label(_) => {
            return Err(errors::invalid(
                "Ground-truth labels cannot enter the observation writer queue",
            ));
        }
    }
    Ok(())
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
        Self::open_with_capacity_and_start(store, capacity, || {})
    }

    fn open_with_capacity_and_start<F>(
        store: SegmentStore,
        capacity: usize,
        before_worker: F,
    ) -> Result<Self>
    where
        F: FnOnce() + Send + 'static,
    {
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
            .spawn(move || {
                before_worker();
                recorder_loop(store, worker_shared)
            })?;
        Ok(Self {
            pane_id,
            harness,
            shared,
            worker: Some(worker),
        })
    }

    #[cfg(test)]
    fn open_paused(
        store: SegmentStore,
        capacity: usize,
    ) -> Result<(Self, Arc<std::sync::Barrier>)> {
        let gate = Arc::new(std::sync::Barrier::new(2));
        let worker_gate = Arc::clone(&gate);
        let recorder = Self::open_with_capacity_and_start(store, capacity, move || {
            worker_gate.wait();
        })?;
        Ok((recorder, gate))
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

    pub fn try_record_gap(&self, gap: ObservationGap) -> Result<QueueOutcome> {
        gap.validate()?;
        if gap.pane_id != self.pane_id || gap.harness != self.harness {
            return Err(errors::invalid(
                "Observation gap provenance does not match its recorder",
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
        let outcome = state.queue.push_gap(gap)?;
        wake.notify_one();
        Ok(outcome)
    }

    /// Labels share the observation writer and sequence but may never be
    /// folded into an overflow gap. The projection worker is the sole
    /// producer, so waiting here also prevents later observations from
    /// overtaking a label whose sequence has already been assigned.
    pub fn record_label(&self, label: GroundTruthLabel) -> Result<()> {
        self.record_label_with_timeout(label, LABEL_WRITE_TIMEOUT)
    }

    fn record_label_with_timeout(&self, label: GroundTruthLabel, timeout: Duration) -> Result<()> {
        label.validate()?;
        if label.pane_id != self.pane_id || label.harness != self.harness {
            return Err(errors::invalid(
                "Ground-truth label provenance does not match its recorder",
            ));
        }
        let (lock, wake) = &*self.shared;
        let mut state = lock
            .lock()
            .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(failure) = &state.failure {
                return Err(errors::internal(failure.clone()));
            }
            if state.queue.closed {
                return Err(errors::internal("Observation recorder is closed"));
            }
            if state.queue.gap.is_none() && state.queue.rows.len() < state.queue.capacity {
                state.queue.push_row(RecordedRow::Label(label))?;
                wake.notify_one();
                return Ok(());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                state.failure = Some(LABEL_WRITE_TIMEOUT_MESSAGE.into());
                state.queue.closed = true;
                wake.notify_all();
                return Err(errors::internal(LABEL_WRITE_TIMEOUT_MESSAGE));
            };
            let (next, _) = wake
                .wait_timeout(state, remaining)
                .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
            state = next;
        }
    }

    pub fn failure(&self) -> Result<Option<String>> {
        let (lock, _) = &*self.shared;
        Ok(lock
            .lock()
            .map_err(|_| errors::internal("Observation queue lock was poisoned"))?
            .failure
            .clone())
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
        let rows = next_recorded_rows(&shared)?;
        let Some(rows) = rows else {
            return store.close();
        };
        if let Err(error) = store.append_batch(rows) {
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

fn next_recorded_rows(shared: &SharedRecorder) -> Result<Option<Vec<RecordedRow>>> {
    let (lock, wake) = &**shared;
    let mut state = lock
        .lock()
        .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
    let mut deadline = None;
    loop {
        if state.queue.gap.is_some() || state.queue.pending() >= OBSERVATION_WRITE_BATCH {
            let rows = state.queue.drain_batch();
            wake.notify_all();
            return Ok(Some(rows));
        }
        if state.queue.closed {
            let rows = (state.queue.pending() > 0).then(|| state.queue.drain_batch());
            wake.notify_all();
            return Ok(rows);
        }
        if state.queue.pending() > 0 {
            let deadline =
                *deadline.get_or_insert_with(|| Instant::now() + OBSERVATION_WRITE_MAX_WAIT);
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                let rows = state.queue.drain_batch();
                wake.notify_all();
                return Ok(Some(rows));
            };
            let (next, timeout) = wake
                .wait_timeout(state, remaining)
                .map_err(|_| errors::internal("Observation queue lock was poisoned"))?;
            state = next;
            if timeout.timed_out() && state.queue.pending() > 0 {
                let rows = state.queue.drain_batch();
                wake.notify_all();
                return Ok(Some(rows));
            }
            continue;
        }
        deadline = None;
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
    appendable: bool,
    last_timestamp_ms: i64,
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
        self.append_batch([row])
    }

    pub fn append_batch(&mut self, rows: impl IntoIterator<Item = RecordedRow>) -> Result<()> {
        let mut wrote = false;
        for row in rows {
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
            let footer = self.encoded_segment_end(timestamp_ms, sequence)?;
            if self.needs_rotation(encoded.len(), footer.len()) {
                self.close_current()?;
                self.create_segment(timestamp_ms)?;
            }
            self.append_bytes(&encoded)?;
            let segment = self
                .segments
                .back_mut()
                .ok_or_else(|| errors::internal("Observation segment is missing"))?;
            segment.last_timestamp_ms = timestamp_ms;
            self.last_sequence = Some(sequence);
            wrote = true;
        }
        if wrote {
            self.sync_current()?;
            self.enforce_retention()?;
        }
        Ok(())
    }

    pub fn next_sequence(&self) -> Result<u64> {
        self.last_sequence
            .map(|sequence| {
                sequence
                    .checked_add(1)
                    .ok_or_else(|| errors::internal("Observation sequence exhausted"))
            })
            .transpose()
            .map(|next| next.unwrap_or(0))
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

    pub fn close(&mut self) -> Result<()> {
        self.close_current()
    }

    fn needs_rotation(&self, row_bytes: usize, footer_bytes: usize) -> bool {
        self.segments.back().is_none_or(|segment| {
            !segment.appendable
                || segment.metadata.harness != self.harness
                || segment.metadata.engine != self.engine
                || segment
                    .size
                    .saturating_add(row_bytes)
                    .saturating_add(footer_bytes)
                    > MAX_SEGMENT_BYTES
        })
    }

    fn encoded_segment_end(&self, ended_at_ms: i64, last_sequence: u64) -> Result<Vec<u8>> {
        Ok(encode_row(&DatasetRow::SegmentEnd(SegmentEnd {
            pane_id: self.pane_id.clone(),
            harness: self.harness.clone(),
            ended_at_ms,
            last_sequence,
        }))?)
    }

    fn close_current(&mut self) -> Result<()> {
        let Some(segment) = self.segments.back() else {
            return Ok(());
        };
        if !segment.appendable {
            return Ok(());
        }
        let last_sequence = self
            .last_sequence
            .ok_or_else(|| errors::internal("Observation segment has no final sequence"))?;
        let footer = self.encoded_segment_end(segment.last_timestamp_ms, last_sequence)?;
        self.append_bytes(&footer)?;
        self.sync_current()?;
        self.segments
            .back_mut()
            .ok_or_else(|| errors::internal("Observation segment disappeared"))?
            .appendable = false;
        Ok(())
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
            appendable: true,
            last_timestamp_ms: started_at_ms,
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
        segment.size = segment.size.saturating_add(bytes.len());
        Ok(())
    }

    fn sync_current(&self) -> Result<()> {
        let segment = self
            .segments
            .back()
            .ok_or_else(|| errors::internal("Observation segment is missing"))?;
        let path = self.directory.join(segment_filename(segment.number));
        let file = OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        validate_private_file(&file)?;
        let actual = usize::try_from(file.metadata()?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        if actual != segment.size {
            return Err(errors::internal(
                "Observation segment changed outside the recorder",
            ));
        }
        file.sync_all()?;
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

pub fn installed_engine_build() -> Result<EngineBuild> {
    let executable = std::env::current_exe()?;
    let mut file = File::open(executable)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    let build = EngineBuild {
        package_version: env!("CARGO_PKG_VERSION").into(),
        executable_sha256: format!("{digest:x}"),
    };
    build.validate()?;
    Ok(build)
}

pub fn experiment_root(profile: &Path) -> Result<PathBuf> {
    let experiments = profile.join("experiments");
    ensure_private_directory(&experiments)?;
    let root = experiments.join("pane-state");
    ensure_private_directory(&root)?;
    Ok(root)
}

pub(crate) fn replay_pane(root: &Path, pane_id: &str) -> Result<Vec<ReplayedSegment>> {
    if pane_id.is_empty() {
        return Err(errors::invalid(
            "Pane identity is required for observation replay",
        ));
    }
    validate_private_directory(root)?;
    let directory = root.join(pane_directory_name(pane_id));
    match fs::symlink_metadata(&directory) {
        Ok(_) => validate_private_directory(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let mut numbers = segment_numbers(&directory)?;
    numbers.sort_unstable();
    let mut last_sequence = None;
    numbers
        .into_iter()
        .map(|number| {
            let segment = read_segment(&directory, number)?;
            validate_loaded_segment(&segment, pane_id, number, &mut last_sequence)?;
            Ok(segment)
        })
        .collect()
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
        let last_timestamp_ms = replayed
            .clean_close
            .as_ref()
            .map(|end| end.ended_at_ms)
            .or_else(|| replayed.rows.last().map(|row| row_identity(row).2))
            .unwrap_or(replayed.metadata.started_at_ms);
        let size = usize::try_from(fs::metadata(directory.join(segment_filename(number)))?.len())
            .map_err(|_| errors::internal("Observation segment size is invalid"))?;
        segments.push_back(Segment {
            number,
            size,
            metadata: replayed.metadata,
            appendable: false,
            last_timestamp_ms,
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
    use soyeht_pane_state::{
        MAX_SCREEN_LINE_BYTES, MAX_SCREEN_LINES, ObservationFrame, ObservationReason,
    };
    use soyeht_protocol::{PaneState, StateSource};
    use std::os::unix::fs::symlink;
    use std::sync::mpsc;
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
            reason: ObservationReason::VisualChange,
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

    fn byte_gap(sequence: u64, dropped_bytes: u64) -> ObservationGap {
        ObservationGap {
            pane_id: "pane-1".into(),
            harness: harness(),
            first_timestamp_ms: 1_800_000_000_000 + sequence as i64,
            last_timestamp_ms: 1_800_000_000_000 + sequence as i64,
            first_sequence: sequence,
            last_sequence: sequence,
            dropped_observations: None,
            dropped_bytes: Some(dropped_bytes),
        }
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
        assert_eq!(gap.dropped_observations, Some(4));
        assert_eq!(gap.dropped_bytes, None);
        assert!(gap.last_timestamp_ms < gap.first_timestamp_ms);
        gap.validate().unwrap();
        assert_eq!(queue.push(frame(4)).unwrap(), QueueOutcome::Queued);
        assert_eq!(queue.pop(), Some(observation(4, false)));
    }

    #[test]
    fn writer_pressure_preserves_both_projected_and_raw_loss_units() {
        let mut queue = ObservationQueue::new(1);
        assert_eq!(queue.push(frame(0)).unwrap(), QueueOutcome::Queued);
        assert_eq!(
            queue.push_gap(byte_gap(1, 4_200_000)).unwrap(),
            QueueOutcome::GapExtended
        );
        let Some(RecordedRow::Gap(gap)) = queue.pop() else {
            panic!("mixed pressure did not leave a gap")
        };
        assert_eq!(gap.first_sequence, 0);
        assert_eq!(gap.last_sequence, 1);
        assert_eq!(gap.dropped_observations, Some(1));
        assert_eq!(gap.dropped_bytes, Some(4_200_000));
        gap.validate().unwrap();
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
        assert!(replayed.iter().all(|segment| segment.clean_close.is_some()));
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
    fn saturated_real_recorder_persists_the_declared_loss_as_its_next_row() {
        let fixture = Fixture::new();
        let store = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        let (recorder, writer_gate) = ObservationRecorder::open_paused(store, 2).unwrap();
        assert_eq!(recorder.try_record(frame(0)).unwrap(), QueueOutcome::Queued);
        assert_eq!(recorder.try_record(frame(1)).unwrap(), QueueOutcome::Queued);
        assert_eq!(
            recorder.try_record(frame(2)).unwrap(),
            QueueOutcome::GapExtended
        );
        writer_gate.wait();
        recorder.finish().unwrap();

        let replayed = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a'))
            .unwrap()
            .replay()
            .unwrap();
        assert_eq!(replayed.len(), 1);
        assert!(replayed[0].clean_close.is_some());
        let [RecordedRow::Gap(gap)] = replayed[0].rows.as_slice() else {
            panic!("saturated recorder did not make its loss the next durable row")
        };
        assert_eq!(gap.first_sequence, 0);
        assert_eq!(gap.last_sequence, 2);
        assert_eq!(gap.dropped_observations, Some(3));
        assert_eq!(gap.dropped_bytes, None);
    }

    #[test]
    fn a_label_waits_for_a_saturated_writer_and_is_never_folded_into_its_gap() {
        let fixture = Fixture::new();
        let store = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        let (recorder, writer_gate) = ObservationRecorder::open_paused(store, 2).unwrap();
        recorder.try_record(frame(0)).unwrap();
        recorder.try_record(frame(1)).unwrap();
        assert_eq!(
            recorder.try_record(frame(2)).unwrap(),
            QueueOutcome::GapExtended
        );
        let label = GroundTruthLabel {
            pane_id: "pane-1".into(),
            harness: harness(),
            timestamp_ms: 1_800_000_000_003,
            sequence: 3,
            state: PaneState::WaitingForYou,
            source: StateSource::Hook,
        };
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (result_sender, result_receiver) = mpsc::sync_channel(1);

        thread::scope(|scope| {
            scope.spawn(|| {
                started_sender.send(()).unwrap();
                result_sender.send(recorder.record_label(label)).unwrap();
            });
            started_receiver.recv().unwrap();
            assert!(
                matches!(result_receiver.try_recv(), Err(mpsc::TryRecvError::Empty)),
                "label bypassed the pending loss row"
            );
            writer_gate.wait();
            result_receiver.recv().unwrap().unwrap();
        });
        recorder.finish().unwrap();

        let rows = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a'))
            .unwrap()
            .replay()
            .unwrap()
            .into_iter()
            .flat_map(|segment| segment.rows)
            .collect::<Vec<_>>();
        let [RecordedRow::Gap(gap), RecordedRow::Label(label)] = rows.as_slice() else {
            panic!("writer pressure did not preserve the gap before the label")
        };
        assert_eq!((gap.first_sequence, gap.last_sequence), (0, 2));
        assert_eq!(gap.dropped_observations, Some(3));
        assert_eq!(label.sequence, 3);
        assert_eq!(label.state, PaneState::WaitingForYou);
        assert_eq!(label.source, StateSource::Hook);
    }

    #[test]
    fn a_label_deadline_closes_a_slow_writer_instead_of_stalling_silently() {
        let fixture = Fixture::new();
        let store = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        let (recorder, writer_gate) = ObservationRecorder::open_paused(store, 1).unwrap();
        recorder.try_record(frame(0)).unwrap();
        let label = GroundTruthLabel {
            pane_id: "pane-1".into(),
            harness: harness(),
            timestamp_ms: 1_800_000_000_001,
            sequence: 1,
            state: PaneState::Working,
            source: StateSource::Protocol,
        };

        let error = recorder
            .record_label_with_timeout(label, Duration::from_millis(10))
            .unwrap_err();
        assert!(error.to_string().contains(LABEL_WRITE_TIMEOUT_MESSAGE));
        assert_eq!(
            recorder.failure().unwrap().as_deref(),
            Some(LABEL_WRITE_TIMEOUT_MESSAGE)
        );
        assert!(recorder.try_record(frame(2)).is_err());

        writer_gate.wait();
        recorder.finish().unwrap();
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
        assert!(
            replayed[..replayed.len() - 1]
                .iter()
                .all(|segment| segment.clean_close.is_some())
        );
        assert_eq!(replayed.last().unwrap().clean_close, None);
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
        assert_eq!(replayed[0].clean_close, None);
        assert_eq!(replayed[1].clean_close, None);
    }

    #[test]
    fn clean_close_and_abrupt_end_remain_distinguishable_after_reopen() {
        let fixture = Fixture::new();
        let store = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        let recorder = ObservationRecorder::open(store).unwrap();
        recorder.try_record(frame(0)).unwrap();
        recorder.finish().unwrap();
        let clean = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a'))
            .unwrap()
            .replay()
            .unwrap();
        assert_eq!(clean.len(), 1);
        assert_eq!(clean[0].clean_close.as_ref().unwrap().last_sequence, 0);

        let mut abrupt =
            SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a')).unwrap();
        abrupt.append(observation(1, false)).unwrap();
        drop(abrupt);
        let replayed = SegmentStore::open(&fixture.root, "pane-1", harness(), engine('a'))
            .unwrap()
            .replay()
            .unwrap();
        assert_eq!(replayed.len(), 2);
        assert!(replayed[0].clean_close.is_some());
        assert_eq!(replayed[1].clean_close, None);
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
