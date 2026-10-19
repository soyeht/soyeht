use crate::errors::{self, Result};
use soyeht_pane_state::{
    GroundTruthLabel, HarnessIdentity, ObservationFrame, ObservationGap, ObservationPaneKind,
    RecordedRow,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

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

mod store;
pub(crate) use store::replay_pane;
use store::row_identity;
pub use store::{DeletionReceipt, SegmentStore, delete_pane, experiment_root};

#[cfg(test)]
mod tests;
