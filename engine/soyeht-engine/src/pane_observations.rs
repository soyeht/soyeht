use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, GroundTruthLabel, HarnessIdentity,
    MAX_RETAINED_SEGMENTS, MAX_SEGMENT_BYTES, ObservationFrame, ObservationGap,
    ObservationPaneKind, RecordedRow, ReplayedSegment, SegmentEnd, SegmentMetadata, decode_segment,
    encode_row, segment_schema_version,
};
use soyeht_platform::fs::{
    OpenOptionsPrivateExt, create_private_dir, dir_is_owner_only, file_is_owner_only,
    sync_directory,
};
use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SEGMENT_SUFFIX: &str = ".jsonl";
pub const OBSERVATION_QUEUE_CAPACITY: usize = 32;
const OBSERVATION_WRITE_BATCH: usize = 8;
const OBSERVATION_WRITE_MAX_WAIT_MS: u64 = 50;
const OBSERVATION_WRITE_MAX_WAIT: Duration = Duration::from_millis(OBSERVATION_WRITE_MAX_WAIT_MS);
const LABEL_WRITE_TIMEOUT_SECONDS: u64 = 30;
const LABEL_WRITE_TIMEOUT_WINDOWS: u64 = 600;
pub(crate) const LABEL_WRITE_TIMEOUT: Duration = Duration::from_secs(LABEL_WRITE_TIMEOUT_SECONDS);
const _: () = assert!(
    LABEL_WRITE_TIMEOUT_SECONDS * 1_000
        == OBSERVATION_WRITE_MAX_WAIT_MS * LABEL_WRITE_TIMEOUT_WINDOWS
);

pub(crate) fn label_write_timeout_message() -> String {
    format!(
        "The observation writer stayed too slow to accept a ground-truth label for {} seconds",
        LABEL_WRITE_TIMEOUT.as_secs()
    )
}

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
        let (_, _, _, _, sequence) = row_identity(&row);
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
            let (pane_id, pane_kind, harness, first_timestamp_ms, first_sequence) =
                row_start(first);
            let pane_id = pane_id.to_owned();
            let harness = harness.clone();
            let mut gap = ObservationGap {
                pane_id,
                pane_kind,
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

fn row_start(row: &RecordedRow) -> (&str, ObservationPaneKind, &HarnessIdentity, i64, u64) {
    match row {
        RecordedRow::Observation(frame) => (
            &frame.pane_id,
            frame.pane_kind,
            &frame.harness,
            frame.timestamp_ms,
            frame.sequence,
        ),
        RecordedRow::Label(label) => (
            &label.pane_id,
            label.pane_kind,
            &label.harness,
            label.timestamp_ms,
            label.sequence,
        ),
        RecordedRow::Gap(gap) => (
            &gap.pane_id,
            gap.pane_kind,
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
    let (pane_id, pane_kind, harness, timestamp_ms, sequence) = row_identity(row);
    if pane_id != gap.pane_id || pane_kind != gap.pane_kind || harness != &gap.harness {
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
    pane_kind: ObservationPaneKind,
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
        let pane_kind = store.pane_kind;
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
            pane_kind,
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
        if frame.pane_id != self.pane_id
            || frame.pane_kind != self.pane_kind
            || frame.harness != self.harness
        {
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
        if gap.pane_id != self.pane_id
            || gap.pane_kind != self.pane_kind
            || gap.harness != self.harness
        {
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
        if label.pane_id != self.pane_id
            || label.pane_kind != self.pane_kind
            || label.harness != self.harness
        {
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
                let failure = label_write_timeout_message();
                state.failure = Some(failure.clone());
                state.queue.closed = true;
                wake.notify_all();
                return Err(errors::internal(failure));
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
        errors::log_if_failed("engine.observations_lost", self.stop_worker());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedSegment {
    pub pane_id: String,
    pub schema_version: u32,
}

pub(crate) struct PaneReplay {
    pub segments: Vec<ReplayedSegment>,
    pub skipped_segments: Vec<SkippedSegment>,
}

enum SegmentRead {
    Current(Box<ReplayedSegment>),
    UnsupportedSchema(u32),
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
    pane_kind: ObservationPaneKind,
    harness: HarnessIdentity,
    engine: EngineBuild,
    segments: VecDeque<Segment>,
    last_sequence: Option<u64>,
    next_segment_number: u64,
}

impl SegmentStore {
    pub fn open(
        root: &Path,
        pane_id: impl Into<String>,
        pane_kind: ObservationPaneKind,
        harness: HarnessIdentity,
        engine: EngineBuild,
    ) -> Result<Self> {
        let pane_id = pane_id.into();
        validate_identity(&pane_id, pane_kind, &harness, &engine)?;
        ensure_private_directory(root)?;
        let directory = root.join(pane_directory_name(&pane_id));
        ensure_private_directory(&directory)?;
        let (segments, last_sequence, next_segment_number) = load_segments(&directory, &pane_id)?;
        Ok(Self {
            root: root.into(),
            directory,
            pane_id,
            pane_kind,
            harness,
            engine,
            segments,
            last_sequence,
            next_segment_number,
        })
    }

    pub fn append(&mut self, row: RecordedRow) -> Result<()> {
        self.append_batch([row])
    }

    pub fn append_batch(&mut self, rows: impl IntoIterator<Item = RecordedRow>) -> Result<()> {
        let mut wrote = false;
        for row in rows {
            row.validate()?;
            let (pane_id, pane_kind, harness, timestamp_ms, sequence) = row_identity(&row);
            if pane_id != self.pane_id || pane_kind != self.pane_kind || harness != &self.harness {
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
                || segment.metadata.pane_kind != self.pane_kind
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
            pane_kind: self.pane_kind,
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
        let number = self.next_segment_number;
        let next_segment_number = number
            .checked_add(1)
            .ok_or_else(|| errors::internal("Observation segment sequence exhausted"))?;
        let metadata = SegmentMetadata {
            schema_version: DATASET_SCHEMA_VERSION,
            pane_id: self.pane_id.clone(),
            pane_kind: self.pane_kind,
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
        self.next_segment_number = next_segment_number;
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
        let mut file = OpenOptions::new().append(true).open_no_follow(&path)?;
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
        let file = OpenOptions::new().append(true).open_no_follow(&path)?;
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
            sync_directory(&self.directory)?;
        }
        Ok(())
    }
}

pub fn experiment_root(profile: &Path) -> Result<PathBuf> {
    let experiments = profile.join("experiments");
    ensure_private_directory(&experiments)?;
    let root = experiments.join("pane-state");
    ensure_private_directory(&root)?;
    Ok(root)
}

pub(crate) fn replay_pane(root: &Path, pane_id: &str) -> Result<PaneReplay> {
    if pane_id.is_empty() {
        return Err(errors::invalid(
            "Pane identity is required for observation replay",
        ));
    }
    validate_private_directory(root)?;
    let directory = root.join(pane_directory_name(pane_id));
    match fs::symlink_metadata(&directory) {
        Ok(_) => validate_private_directory(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PaneReplay {
                segments: Vec::new(),
                skipped_segments: Vec::new(),
            });
        }
        Err(error) => return Err(error.into()),
    }
    let mut numbers = segment_numbers(&directory)?;
    numbers.sort_unstable();
    let mut last_sequence = None;
    let mut segments = Vec::new();
    let mut skipped_segments = Vec::new();
    for number in numbers {
        match read_segment_entry(&directory, number)? {
            SegmentRead::Current(segment) => {
                let segment = *segment;
                validate_loaded_segment(&segment, pane_id, number, &mut last_sequence)?;
                segments.push(segment);
            }
            SegmentRead::UnsupportedSchema(schema_version) => {
                skipped_segments.push(SkippedSegment {
                    pane_id: pane_id.to_owned(),
                    schema_version,
                });
            }
        }
    }
    Ok(PaneReplay {
        segments,
        skipped_segments,
    })
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
    sync_directory(&directory)?;
    fs::remove_dir(&directory)?;
    sync_directory(root)?;
    Ok(receipt)
}

fn validate_identity(
    pane_id: &str,
    pane_kind: ObservationPaneKind,
    harness: &HarnessIdentity,
    engine: &EngineBuild,
) -> Result<()> {
    let metadata = SegmentMetadata {
        schema_version: DATASET_SCHEMA_VERSION,
        pane_id: pane_id.into(),
        pane_kind,
        harness: harness.clone(),
        engine: engine.clone(),
        segment_sequence: 0,
        started_at_ms: 0,
    };
    Ok(metadata.validate()?)
}

fn load_segments(directory: &Path, pane_id: &str) -> Result<(VecDeque<Segment>, Option<u64>, u64)> {
    let mut numbers = segment_numbers(directory)?;
    numbers.sort_unstable();
    let next_segment_number = numbers
        .last()
        .map(|number| {
            number
                .checked_add(1)
                .ok_or_else(|| errors::internal("Observation segment sequence exhausted"))
        })
        .transpose()?
        .unwrap_or(0);
    let mut segments = VecDeque::new();
    let mut last_sequence = None;
    for number in numbers {
        let SegmentRead::Current(replayed) = read_segment_entry(directory, number)? else {
            continue;
        };
        let replayed = *replayed;
        validate_loaded_segment(&replayed, pane_id, number, &mut last_sequence)?;
        let last_timestamp_ms = replayed
            .clean_close
            .as_ref()
            .map(|end| end.ended_at_ms)
            .or_else(|| replayed.rows.last().map(|row| row_identity(row).3))
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
    Ok((segments, last_sequence, next_segment_number))
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
        let sequence = row_identity(row).4;
        if last_sequence.is_some_and(|last| sequence <= last) {
            return Err(soyeht_pane_state::Error::InvalidSequence.into());
        }
        *last_sequence = Some(sequence);
    }
    Ok(())
}

fn read_segment(directory: &Path, number: u64) -> Result<ReplayedSegment> {
    match read_segment_entry(directory, number)? {
        SegmentRead::Current(segment) => Ok(*segment),
        SegmentRead::UnsupportedSchema(_) => Err(soyeht_pane_state::Error::SchemaVersion.into()),
    }
}

fn read_segment_entry(directory: &Path, number: u64) -> Result<SegmentRead> {
    let path = directory.join(segment_filename(number));
    let mut file = OpenOptions::new().read(true).open_no_follow(&path)?;
    validate_private_file(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SEGMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let schema_version = segment_schema_version(&bytes)?;
    if schema_version != DATASET_SCHEMA_VERSION {
        return Ok(SegmentRead::UnsupportedSchema(schema_version));
    }
    Ok(SegmentRead::Current(Box::new(decode_segment(&bytes)?)))
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
        .owner_only()
        .open_no_follow(&path)?;
    file.write_all(header)?;
    file.sync_all()?;
    sync_directory(directory)?;
    Ok(())
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_private_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_dir(path)?;
            validate_private_directory(path)
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || !dir_is_owner_only(path)? {
        return Err(errors::internal(
            "Observation directory is not private and symlink-free",
        ));
    }
    Ok(())
}

fn validate_private_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || !file_is_owner_only(file)? {
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

fn row_identity(row: &RecordedRow) -> (&str, ObservationPaneKind, &HarnessIdentity, i64, u64) {
    match row {
        RecordedRow::Observation(frame) => (
            &frame.pane_id,
            frame.pane_kind,
            &frame.harness,
            frame.timestamp_ms,
            frame.sequence,
        ),
        RecordedRow::Label(label) => (
            &label.pane_id,
            label.pane_kind,
            &label.harness,
            label.timestamp_ms,
            label.sequence,
        ),
        RecordedRow::Gap(gap) => (
            &gap.pane_id,
            gap.pane_kind,
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
mod tests;
