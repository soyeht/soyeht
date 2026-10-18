use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{
        DeletionReceipt, LABEL_WRITE_TIMEOUT, ObservationRecorder, SegmentStore, delete_pane,
        experiment_root,
    },
    terminal_projection::TerminalProjection,
};
use crossbeam_channel as channel;
use soyeht_pane_state::{EngineBuild, GroundTruthProvenance, HarnessIdentity, ObservationPaneKind};
use soyeht_protocol::{PaneExperimentCapture, PaneMode, PaneState, StateSource, method};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CAPTURE_FAILURE: &str = "Experiment capture stopped because its private recorder failed";
const PROJECTION_QUEUE_CAPACITY: usize = 32;
const FAILURE_CHECK_INTERVAL: Duration = Duration::from_millis(50);

#[cfg(test)]
struct TestQueueMetrics {
    enabled: AtomicBool,
    outstanding: std::sync::atomic::AtomicUsize,
    peak_outstanding: std::sync::atomic::AtomicUsize,
    processed: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl Default for TestQueueMetrics {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            outstanding: std::sync::atomic::AtomicUsize::new(0),
            peak_outstanding: std::sync::atomic::AtomicUsize::new(0),
            processed: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
#[derive(Default)]
struct TestProjectionGate {
    paused: Mutex<bool>,
    wake: std::sync::Condvar,
}

#[cfg(test)]
impl TestProjectionGate {
    fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
        if !paused {
            self.wake.notify_all();
        }
    }

    fn wait(&self) {
        let mut paused = self.paused.lock().unwrap();
        while *paused {
            paused = self.wake.wait(paused).unwrap();
        }
    }
}

enum ProjectionCommand {
    Bytes {
        timestamp_ms: i64,
        bytes: Arc<Vec<u8>>,
        #[cfg(test)]
        counted: bool,
    },
    Resize {
        rows: u16,
        cols: u16,
    },
    Label {
        timestamp_ms: i64,
        state: PaneState,
        source: StateSource,
        provenance: GroundTruthProvenance,
        reply: channel::Sender<std::result::Result<(), String>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawByteGap {
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    dropped_bytes: u64,
}

impl RawByteGap {
    fn new(timestamp_ms: i64, bytes: usize) -> Result<Self> {
        let dropped_bytes = u64::try_from(bytes)
            .map_err(|_| errors::internal("Terminal byte count exceeds the supported range"))?;
        if timestamp_ms < 0 || dropped_bytes == 0 {
            return Err(errors::invalid("Terminal byte gap is invalid"));
        }
        Ok(Self {
            first_timestamp_ms: timestamp_ms,
            last_timestamp_ms: timestamp_ms,
            dropped_bytes,
        })
    }

    fn extend(&mut self, timestamp_ms: i64, bytes: usize) -> Result<()> {
        let bytes = u64::try_from(bytes)
            .map_err(|_| errors::internal("Terminal byte count exceeds the supported range"))?;
        if timestamp_ms < 0 || bytes == 0 {
            return Err(errors::invalid("Terminal byte gap is invalid"));
        }
        self.last_timestamp_ms = timestamp_ms;
        self.dropped_bytes = self
            .dropped_bytes
            .checked_add(bytes)
            .ok_or_else(|| errors::internal("Terminal byte gap exceeds the supported range"))?;
        Ok(())
    }
}

pub(crate) struct TerminalCapture {
    sender: Option<channel::Sender<ProjectionCommand>>,
    overflow: Arc<Mutex<Option<RawByteGap>>>,
    overflow_active: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
    #[cfg(test)]
    test_queue_metrics: Arc<TestQueueMetrics>,
    #[cfg(test)]
    test_projection_gate: Arc<TestProjectionGate>,
}

impl TerminalCapture {
    fn open(
        root: &Path,
        pane_id: &str,
        harness: HarnessIdentity,
        session_generation: u32,
        rows: u32,
        cols: u32,
        build: EngineBuild,
    ) -> Result<Self> {
        let rows = u16::try_from(rows)
            .map_err(|_| errors::invalid("Pane rows exceed the observation range"))?;
        let cols = u16::try_from(cols)
            .map_err(|_| errors::invalid("Pane columns exceed the observation range"))?;
        let store = SegmentStore::open(
            root,
            pane_id,
            ObservationPaneKind::Terminal,
            harness.clone(),
            build,
        )?;
        let sequence = store.next_sequence()?;
        let projection = TerminalProjection::new(
            pane_id,
            ObservationPaneKind::Terminal,
            harness,
            session_generation,
            rows,
            cols,
            sequence,
        )?;
        let recorder = ObservationRecorder::open(store)?;
        let (sender, receiver) = channel::bounded(PROJECTION_QUEUE_CAPACITY);
        let overflow = Arc::new(Mutex::new(None));
        let overflow_active = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let worker_overflow = Arc::clone(&overflow);
        let worker_overflow_active = Arc::clone(&overflow_active);
        let worker_failure = Arc::clone(&failure);
        #[cfg(test)]
        let test_queue_metrics = Arc::new(TestQueueMetrics::default());
        #[cfg(test)]
        let worker_test_queue_metrics = Arc::clone(&test_queue_metrics);
        #[cfg(test)]
        let test_projection_gate = Arc::new(TestProjectionGate::default());
        #[cfg(test)]
        let worker_test_projection_gate = Arc::clone(&test_projection_gate);
        let worker = thread::Builder::new()
            .name("terminal-observation-projector".into())
            .spawn(move || {
                let Err(error) = projection_loop(
                    projection,
                    recorder,
                    receiver,
                    worker_overflow,
                    worker_overflow_active,
                    #[cfg(test)]
                    worker_test_queue_metrics,
                    #[cfg(test)]
                    worker_test_projection_gate,
                ) else {
                    return;
                };
                let Ok(mut failure) = worker_failure.lock() else {
                    return;
                };
                *failure = Some(error.to_string());
            })?;
        Ok(Self {
            sender: Some(sender),
            overflow,
            overflow_active,
            failure,
            worker: Some(worker),
            #[cfg(test)]
            test_queue_metrics,
            #[cfg(test)]
            test_projection_gate,
        })
    }

    fn record(&self, timestamp_ms: i64, bytes: Arc<Vec<u8>>) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if self.overflow_active.load(Ordering::Acquire) {
            let mut overflow = self
                .overflow
                .lock()
                .map_err(|_| errors::internal("Terminal projection gap lock was poisoned"))?;
            if let Some(gap) = overflow.as_mut() {
                return gap.extend(timestamp_ms, bytes.len());
            }
            self.overflow_active.store(false, Ordering::Release);
        }
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| errors::internal("Terminal projection worker is closed"))?;
        #[cfg(test)]
        let counted = self.test_queue_metrics.enabled.load(Ordering::Relaxed);
        #[cfg(test)]
        if counted {
            let outstanding = self
                .test_queue_metrics
                .outstanding
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            self.test_queue_metrics
                .peak_outstanding
                .fetch_max(outstanding, Ordering::Relaxed);
        }
        let outcome = sender.try_send(ProjectionCommand::Bytes {
            timestamp_ms,
            bytes,
            #[cfg(test)]
            counted,
        });
        #[cfg(test)]
        if outcome.is_err() && counted {
            self.test_queue_metrics
                .outstanding
                .fetch_sub(1, Ordering::Relaxed);
        }
        match outcome {
            Ok(()) => Ok(()),
            Err(channel::TrySendError::Full(ProjectionCommand::Bytes {
                timestamp_ms,
                bytes,
                ..
            })) => {
                *self
                    .overflow
                    .lock()
                    .map_err(|_| errors::internal("Terminal projection gap lock was poisoned"))? =
                    Some(RawByteGap::new(timestamp_ms, bytes.len())?);
                self.overflow_active.store(true, Ordering::Release);
                Ok(())
            }
            Err(channel::TrySendError::Disconnected(_)) => {
                Err(errors::internal("Terminal projection worker stopped"))
            }
            Err(channel::TrySendError::Full(ProjectionCommand::Resize { .. })) => {
                Err(errors::internal("Terminal projection command changed kind"))
            }
            Err(channel::TrySendError::Full(ProjectionCommand::Label { .. })) => {
                Err(errors::internal("Terminal projection command changed kind"))
            }
        }
    }

    fn record_label(
        &self,
        timestamp_ms: i64,
        state: PaneState,
        source: StateSource,
        provenance: GroundTruthProvenance,
    ) -> Result<()> {
        let deadline = Instant::now() + LABEL_WRITE_TIMEOUT;
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| errors::internal("Terminal projection worker is closed"))?;
        let (reply, result) = channel::bounded(1);
        sender
            .send_timeout(
                ProjectionCommand::Label {
                    timestamp_ms,
                    state,
                    source,
                    provenance,
                    reply,
                },
                LABEL_WRITE_TIMEOUT,
            )
            .map_err(|_| {
                errors::internal("Terminal projection stayed too slow to accept a label")
            })?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        result
            .recv_timeout(remaining)
            .map_err(|_| errors::internal("Terminal projection stayed too slow to record a label"))?
            .map_err(errors::internal)
    }

    fn resize(&self, rows: u32, cols: u32) -> Result<()> {
        let rows = u16::try_from(rows)
            .map_err(|_| errors::invalid("Pane rows exceed the observation range"))?;
        let cols = u16::try_from(cols)
            .map_err(|_| errors::invalid("Pane columns exceed the observation range"))?;
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| errors::internal("Terminal projection worker is closed"))?;
        sender
            .send(ProjectionCommand::Resize { rows, cols })
            .map_err(|_| errors::internal("Terminal projection worker stopped"))
    }

    fn failure_reason(&self) -> Result<Option<String>> {
        Ok(self
            .failure
            .lock()
            .map_err(|_| errors::internal("Terminal capture failure lock was poisoned"))?
            .clone())
    }
}

impl Drop for TerminalCapture {
    fn drop(&mut self) {
        #[cfg(test)]
        self.test_projection_gate.set_paused(false);
        self.sender.take();
        if let Some(Err(_)) = self.worker.take().map(|worker| worker.join()) {
            soyeht_platform::log::event("engine.terminal_capture_panicked", &[]);
        }
    }
}

fn projection_loop(
    mut projection: TerminalProjection,
    recorder: ObservationRecorder,
    receiver: channel::Receiver<ProjectionCommand>,
    overflow: Arc<Mutex<Option<RawByteGap>>>,
    overflow_active: Arc<AtomicBool>,
    #[cfg(test)] test_queue_metrics: Arc<TestQueueMetrics>,
    #[cfg(test)] test_projection_gate: Arc<TestProjectionGate>,
) -> Result<()> {
    loop {
        match receiver.recv_timeout(FAILURE_CHECK_INTERVAL) {
            Ok(ProjectionCommand::Bytes {
                timestamp_ms,
                bytes,
                #[cfg(test)]
                counted,
            }) => {
                #[cfg(test)]
                test_projection_gate.wait();
                let result: Result<()> = (|| {
                    if let Some(frame) = projection.process(timestamp_ms, &bytes)? {
                        recorder.try_record(frame)?;
                    }
                    Ok(())
                })();
                #[cfg(test)]
                {
                    if counted {
                        test_queue_metrics
                            .outstanding
                            .fetch_sub(1, Ordering::Relaxed);
                    }
                    test_queue_metrics.processed.fetch_add(1, Ordering::Release);
                }
                result?;
            }
            Ok(ProjectionCommand::Resize { rows, cols }) => {
                #[cfg(test)]
                test_projection_gate.wait();
                projection.resize(rows, cols)?;
            }
            Ok(ProjectionCommand::Label {
                timestamp_ms,
                state,
                source,
                provenance,
                reply,
            }) => {
                #[cfg(test)]
                test_projection_gate.wait();
                let result = (|| {
                    flush_gap(&overflow, &overflow_active, &mut projection, &recorder)?;
                    recorder.record_label(projection.label(
                        timestamp_ms,
                        state,
                        source,
                        provenance,
                    )?)
                })();
                let reply_result = result.as_ref().map(|_| ()).map_err(ToString::to_string);
                if reply.send(reply_result).is_err() {
                    // The requester stopped waiting; the outcome still returns below.
                    soyeht_platform::log::event("engine.label_requester_gone", &[]);
                }
                result?;
            }
            Err(channel::RecvTimeoutError::Timeout) => {
                if flush_gap(&overflow, &overflow_active, &mut projection, &recorder)? {
                    continue;
                }
                if let Some(failure) = recorder.failure()? {
                    return Err(errors::internal(failure));
                }
            }
            Err(channel::RecvTimeoutError::Disconnected) => {
                flush_gap(&overflow, &overflow_active, &mut projection, &recorder)?;
                return recorder.finish();
            }
        }
    }
}

/// Records the gap of bytes the handoff dropped, if any. True when there was one.
fn flush_gap(
    overflow: &Mutex<Option<RawByteGap>>,
    overflow_active: &AtomicBool,
    projection: &mut TerminalProjection,
    recorder: &ObservationRecorder,
) -> Result<bool> {
    let gap = overflow
        .lock()
        .map_err(|_| errors::internal("Terminal projection gap lock was poisoned"))?
        .take();
    overflow_active.store(false, Ordering::Release);
    let Some(gap) = gap else { return Ok(false) };
    let record = projection.record_byte_gap(
        gap.first_timestamp_ms,
        gap.last_timestamp_ms,
        gap.dropped_bytes,
    )?;
    recorder.try_record_gap(record)?;
    Ok(true)
}

impl Core {
    pub(crate) fn observation_root(&self) -> Result<std::path::PathBuf> {
        experiment_root(&self.directory)
    }

    pub(crate) fn prepare_terminal_capture(&mut self, pane_id: &str) -> Result<TerminalCapture> {
        let stored = self
            .panes
            .get(pane_id)
            .filter(|pane| !pane.stored.closed && pane.stored.pane.mode == PaneMode::Terminal)
            .ok_or_else(|| errors::not_found("Terminal pane not found"))?
            .stored
            .clone();
        let build = self.capture_build()?;
        let harness = self
            .terminal_harnesses
            .get(pane_id)
            .map_or(HarnessIdentity::Unknown, |session| session.harness.clone());
        TerminalCapture::open(
            &self.observation_root()?,
            pane_id,
            harness,
            stored.pane.generation,
            stored.pane.rows,
            stored.pane.cols,
            build,
        )
    }

    pub(crate) fn delete_experiment_capture_data(
        &mut self,
        connection: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::PANE_EXPERIMENT_DELETE, params)?
        {
            return Ok(result);
        }
        let pane_id = errors::param_str(params, "paneId")?;
        let exports_path = self.observation_root()?.join("exports");
        let exports_path = exports_path
            .to_str()
            .ok_or_else(|| errors::internal("Experiment export path is not valid text"))?;
        let receipt = self.delete_experiment_capture_files(pane_id)?;
        let files = u32::try_from(receipt.files)
            .map_err(|_| errors::internal("Observation deletion file count is invalid"))?;
        let result = serde_json::json!({
            "files":files,
            "bytes":receipt.bytes.to_string(),
            "nothingRemained":true,
            "exportsPath":exports_path
        });
        match self.mutate(connection, method::PANE_EXPERIMENT_DELETE, params, |_tx| {
            Ok((result, vec![]))
        }) {
            Ok(mutation) => Ok(mutation.result),
            Err(error) => Err(deletion_receipt_commit_error(&receipt, exports_path, error)),
        }
    }

    fn delete_experiment_capture_files(&mut self, pane_id: &str) -> Result<DeletionReceipt> {
        let pane = self
            .panes
            .get(pane_id)
            .filter(|pane| {
                !pane.stored.closed
                    && matches!(pane.stored.pane.mode, PaneMode::Terminal | PaneMode::Agent)
            })
            .ok_or_else(|| errors::not_found("Experiment pane not found"))?;
        if pane
            .stored
            .pane
            .experiment_capture
            .as_ref()
            .is_some_and(|capture| capture.on)
            || self.terminal_captures.contains_key(pane_id)
            || self.agent_captures.contains_key(pane_id)
        {
            return Err(errors::pane_busy(
                "Stop experiment capture before deleting its data",
            ));
        }
        delete_pane(&self.observation_root()?, pane_id)
    }

    pub(crate) fn observe_terminal(&mut self, pane_id: &str, bytes: Arc<Vec<u8>>) -> Result<()> {
        let timestamp_ms = self.now_ms();
        let failure = self
            .terminal_captures
            .get_mut(pane_id)
            .and_then(|capture| capture.record(timestamp_ms, bytes).err())
            .map(|error| error.to_string());
        if let Some(failure) = failure {
            self.fail_experiment_capture(pane_id, Some(failure))?;
        }
        Ok(())
    }

    pub(crate) fn record_terminal_hook_label(
        &mut self,
        pane_id: &str,
        state: PaneState,
        credential_sha256: String,
    ) -> Result<()> {
        let generation = self
            .panes
            .get(pane_id)
            .ok_or_else(|| errors::internal("Captured terminal pane disappeared"))?
            .stored
            .pane
            .generation;
        let timestamp_ms = self.now_ms();
        let failure = self
            .terminal_captures
            .get(pane_id)
            .and_then(|capture| {
                capture
                    .record_label(
                        timestamp_ms,
                        state,
                        StateSource::Hook,
                        GroundTruthProvenance::session_hook(generation, credential_sha256),
                    )
                    .err()
            })
            .map(|error| error.to_string());
        if let Some(failure) = failure {
            self.fail_experiment_capture(pane_id, Some(failure))?;
        }
        Ok(())
    }

    pub(crate) fn reconcile_experiment_captures(&mut self) -> Result<()> {
        self.expire_capture_restore()?;
        let mut failed = Vec::new();
        for (pane_id, capture) in &self.terminal_captures {
            if let Some(failure) = capture.failure_reason()? {
                failed.push((pane_id.clone(), failure));
            }
        }
        for (pane_id, capture) in &self.agent_captures {
            if let Some(failure) = capture.failure_reason()? {
                failed.push((pane_id.clone(), failure));
            }
        }
        for (pane_id, failure) in failed {
            self.fail_experiment_capture(&pane_id, Some(failure))?;
        }
        Ok(())
    }

    pub(crate) fn resize_terminal_capture(&mut self, pane_id: &str) -> Result<()> {
        let Some(capture) = self.terminal_captures.get_mut(pane_id) else {
            return Ok(());
        };
        let pane = &self.panes[pane_id].stored.pane;
        if let Err(error) = capture.resize(pane.rows, pane.cols) {
            self.fail_experiment_capture(pane_id, Some(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn fail_experiment_capture(
        &mut self,
        pane_id: &str,
        failure: Option<String>,
    ) -> Result<()> {
        self.terminal_captures.remove(pane_id);
        self.agent_captures.remove(pane_id);
        let mut saved = errors::invariant(
            self.panes.get(pane_id),
            "Captured pane disappeared before failure publication",
        )?
        .stored
        .clone();
        saved.pane.experiment_capture = Some(PaneExperimentCapture {
            on: false,
            failure: Some(
                failure
                    .map(|failure| format!("Experiment capture stopped: {failure}"))
                    .unwrap_or_else(|| CAPTURE_FAILURE.into()),
            ),
        });
        self.save_runtime(pane_id, saved)
    }
}

fn deletion_receipt_commit_error(
    receipt: &DeletionReceipt,
    exports_path: &str,
    error: errors::Error,
) -> errors::Error {
    errors::internal(format!(
        "Captured experiment data was removed ({} files, {} bytes), but the record of its removal was not saved: {}. Export files were not deleted and remain in {}",
        receipt.files, receipt.bytes, error, exports_path
    ))
}

#[cfg(test)]
mod tests;
