use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{
        DeletionReceipt, ObservationRecorder, SegmentStore, delete_pane, experiment_root,
        installed_engine_build,
    },
    terminal_projection::TerminalProjection,
};
use crossbeam_channel as channel;
use soyeht_pane_state::{EngineBuild, HarnessIdentity, ObservationPaneKind};
use soyeht_protocol::{PaneExperimentCapture, PaneMode};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
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
        }
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
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
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
            Err(channel::RecvTimeoutError::Timeout) => {
                if let Some(gap) = take_raw_gap(&overflow, &overflow_active)? {
                    recorder.try_record_gap(projection.record_byte_gap(
                        gap.first_timestamp_ms,
                        gap.last_timestamp_ms,
                        gap.dropped_bytes,
                    )?)?;
                    continue;
                }
                if let Some(failure) = recorder.failure()? {
                    return Err(errors::internal(failure));
                }
            }
            Err(channel::RecvTimeoutError::Disconnected) => {
                if let Some(gap) = take_raw_gap(&overflow, &overflow_active)? {
                    recorder.try_record_gap(projection.record_byte_gap(
                        gap.first_timestamp_ms,
                        gap.last_timestamp_ms,
                        gap.dropped_bytes,
                    )?)?;
                }
                return recorder.finish();
            }
        }
    }
}

fn take_raw_gap(
    overflow: &Mutex<Option<RawByteGap>>,
    overflow_active: &AtomicBool,
) -> Result<Option<RawByteGap>> {
    let gap = overflow
        .lock()
        .map_err(|_| errors::internal("Terminal projection gap lock was poisoned"))?
        .take();
    overflow_active.store(false, Ordering::Release);
    Ok(gap)
}

impl Core {
    pub(crate) fn observation_root(&self) -> Result<std::path::PathBuf> {
        experiment_root(&self.directory)
    }

    pub(crate) fn capture_build(&mut self) -> Result<EngineBuild> {
        if let Some(build) = &self.experiment_capture_build {
            return Ok(build.clone());
        }
        let build = installed_engine_build()?;
        self.experiment_capture_build = Some(build.clone());
        Ok(build)
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
        TerminalCapture::open(
            &self.observation_root()?,
            pane_id,
            HarnessIdentity::Unknown,
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
            self.repeated_mutation(connection, "pane.experiment.delete", params)?
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
        match self.mutate(connection, "pane.experiment.delete", params, |_tx| {
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

    pub(crate) fn restore_experiment_captures(&mut self) -> Result<()> {
        let ids: Vec<_> = self
            .panes
            .iter()
            .filter(|(_, pane)| {
                !pane.stored.closed
                    && matches!(pane.stored.pane.mode, PaneMode::Terminal | PaneMode::Agent)
                    && pane
                        .stored
                        .pane
                        .experiment_capture
                        .as_ref()
                        .is_some_and(|capture| capture.on)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let mode = self.panes[&id].stored.pane.mode.clone();
            let restored = match mode {
                PaneMode::Terminal => self.prepare_terminal_capture(&id).map(|capture| {
                    self.terminal_captures.insert(id.clone(), capture);
                }),
                PaneMode::Agent => self.prepare_agent_capture(&id).map(|capture| {
                    self.agent_captures.insert(id.clone(), capture);
                }),
                _ => unreachable!("experiment capture mode was filtered"),
            };
            if let Err(error) = restored {
                self.fail_experiment_capture(&id, Some(error.to_string()))?;
            }
        }
        Ok(())
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

    pub(crate) fn reconcile_experiment_captures(&mut self) -> Result<()> {
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
mod tests {
    use super::*;
    use crate::{
        dispatch::Outcome,
        model::{RunningPane, StoredPane},
        outbox,
        pane_observations::label_write_timeout_message,
        ports::{
            PortScanObservation, PortScanPort,
            testing::{FakeSupervisor, FixedClock},
        },
        screen::Screen,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use soyeht_pane_state::RecordedRow;
    use soyeht_protocol::{Pane, PaneState, StateSource};
    use soyeht_ptyd::wire::{Output, Response};
    use std::{collections::VecDeque, fs, io, os::unix::fs::PermissionsExt, time::Instant};
    use uuid::Uuid;

    const CADENCE_RUNS: usize = 10;
    const CADENCE_POLLS_PER_SAMPLE: usize = 30;
    const CADENCE_MAX_ATTEMPTS: usize = 10;
    const CADENCE_WARMUP_PAIRS: usize = 3;
    const CADENCE_FIXTURE_BYTES: usize = 13_367_794;
    const CADENCE_STEADY_BYTES: usize = 9_406_002;
    const CADENCE_WARMUP_DRAIN: Duration = Duration::from_millis(60);
    const FRAME_DENSE_INTERVAL: Duration = Duration::from_millis(5);
    const FRAME_DENSE_OBSERVATIONS: usize = 400;

    fn load_average() -> Option<[f64; 3]> {
        let mut averages = [0.0; 3];
        // SAFETY: `averages` remains alive and writable for the call, and the
        // supplied element count exactly matches the three-element buffer.
        let count = unsafe { libc::getloadavg(averages.as_mut_ptr(), averages.len() as i32) };
        (count == averages.len() as i32).then_some(averages)
    }

    fn log_cadence_attempt(
        attempt: usize,
        before: Option<[f64; 3]>,
        after: Option<[f64; 3]>,
        off: &[u128],
        on: &[u128],
    ) {
        eprintln!(
            "capture cadence attempt={attempt} load_average_before={before:?} load_average_after={after:?} off_us={off:?} on_us={on:?}"
        );
    }

    fn cadence_samples_pass(
        attempt: usize,
        load_average_before: Option<[f64; 3]>,
        off_micros: &[u128],
        on_micros: &[u128],
    ) -> bool {
        let steady_off = &off_micros[CADENCE_WARMUP_PAIRS..];
        let steady_on = &on_micros[CADENCE_WARMUP_PAIRS..];
        log_cadence_attempt(
            attempt,
            load_average_before,
            load_average(),
            off_micros,
            on_micros,
        );
        if !capture_is_not_faster(steady_off, steady_on) {
            eprintln!(
                "capture cadence attempt={attempt} invalid: capture-on measured faster than capture-off after the declared warm-up"
            );
            return false;
        }
        assert!(percentile(steady_on, 1, 2) * 100 <= percentile(steady_off, 1, 2) * 105);
        assert!(percentile(steady_on, 95, 100) <= percentile(steady_off, 95, 100) + 5_000);
        true
    }

    fn time_coverage_basis_points(
        first_timestamp_ms: i64,
        last_timestamp_ms: i64,
        gaps: impl Iterator<Item = (i64, i64)>,
    ) -> u64 {
        let session_ms =
            u64::try_from(last_timestamp_ms.saturating_sub(first_timestamp_ms)).unwrap_or_default();
        let gap_ms = gaps.fold(0_u64, |total, (first, last)| {
            total.saturating_add(u64::try_from(last.saturating_sub(first)).unwrap_or_default())
        });
        session_ms
            .saturating_sub(gap_ms)
            .saturating_mul(10_000)
            .checked_div(session_ms)
            .unwrap_or(10_000)
    }

    fn recorded_time_coverage_basis_points(rows: &[&RecordedRow]) -> u64 {
        let first_timestamp = rows
            .iter()
            .map(|row| match row {
                RecordedRow::Observation(frame) => frame.timestamp_ms,
                RecordedRow::Gap(gap) => gap.first_timestamp_ms,
                RecordedRow::Label(label) => label.timestamp_ms,
            })
            .min()
            .unwrap();
        let last_timestamp = rows
            .iter()
            .map(|row| match row {
                RecordedRow::Observation(frame) => frame.timestamp_ms,
                RecordedRow::Gap(gap) => gap.last_timestamp_ms,
                RecordedRow::Label(label) => label.timestamp_ms,
            })
            .max()
            .unwrap();
        time_coverage_basis_points(
            first_timestamp,
            last_timestamp,
            rows.iter().filter_map(|row| match row {
                RecordedRow::Gap(gap) => Some((gap.first_timestamp_ms, gap.last_timestamp_ms)),
                _ => None,
            }),
        )
    }

    fn assert_complete_cadence_coverage(observations: usize, gaps: usize, coverage: u64) {
        assert_eq!(observations, CADENCE_RUNS * CADENCE_POLLS_PER_SAMPLE + 1);
        assert_eq!(gaps, 0);
        assert_eq!(coverage, 10_000);
    }

    #[test]
    fn time_coverage_is_derived_from_the_recorded_gap_intervals() {
        assert_eq!(time_coverage_basis_points(0, 100, [].into_iter()), 10_000);
        assert_eq!(
            time_coverage_basis_points(0, 100, [(20, 30), (50, 60)].into_iter()),
            8_000
        );
    }

    struct EmptyPortScan;

    impl PortScanPort for EmptyPortScan {
        fn scan(&mut self) -> io::Result<PortScanObservation> {
            Ok(PortScanObservation {
                listeners: Vec::new(),
                directories: Vec::new(),
            })
        }
    }

    #[test]
    fn full_projection_handoff_coalesces_exact_bytes_without_failing_capture() {
        let (sender, receiver) = channel::bounded(1);
        let overflow = Arc::new(Mutex::new(None));
        let capture = TerminalCapture {
            sender: Some(sender),
            overflow: Arc::clone(&overflow),
            overflow_active: Arc::new(AtomicBool::new(false)),
            failure: Arc::new(Mutex::new(None)),
            worker: None,
            test_queue_metrics: Arc::new(TestQueueMetrics::default()),
            test_projection_gate: Arc::new(TestProjectionGate::default()),
        };
        capture.record(10, vec![b'a'].into()).unwrap();
        capture.record(20, vec![b'b'; 4].into()).unwrap();
        capture.record(30, vec![b'c'; 7].into()).unwrap();

        let gap = overflow.lock().unwrap().clone().unwrap();
        assert_eq!(gap.first_timestamp_ms, 20);
        assert_eq!(gap.last_timestamp_ms, 30);
        assert_eq!(gap.dropped_bytes, 11);
        assert_eq!(capture.failure_reason().unwrap(), None);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ProjectionCommand::Bytes {
                timestamp_ms: 10,
                ..
            }
        ));
        drop(receiver);
    }

    #[test]
    fn a_slow_observation_writer_reason_reaches_the_durable_pane_state() {
        let directory = std::env::temp_dir().join(format!("sn-capture-failure-{}", Uuid::new_v4()));
        let mut core = cadence_core(&directory, &[], true);
        let failure = Arc::clone(&core.terminal_captures["cadence-pane"].failure);
        let reason = label_write_timeout_message();
        *failure.lock().unwrap() = Some(reason.clone());

        core.reconcile_experiment_captures().unwrap();

        assert!(!core.terminal_captures.contains_key("cadence-pane"));
        assert_eq!(
            core.panes["cadence-pane"]
                .stored
                .pane
                .experiment_capture
                .as_ref(),
            Some(&PaneExperimentCapture {
                on: false,
                failure: Some(format!("Experiment capture stopped: {reason}")),
            })
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn pane_capture_data_deletes_only_after_its_worker_stops_and_returns_a_receipt() {
        let directory = std::env::temp_dir().join(format!("sn-delete-capture-{}", Uuid::new_v4()));
        let mut core = cadence_core(&directory, &[], false);
        let pane_id = "cadence-pane";
        let capture = core.prepare_terminal_capture(pane_id).unwrap();
        capture
            .record(
                1,
                b"\x1b[2J\x1b[Hcaptured terminal text\r\n".to_vec().into(),
            )
            .unwrap();
        drop(capture);

        let active = core.prepare_terminal_capture(pane_id).unwrap();
        core.terminal_captures.insert(pane_id.into(), active);
        let busy = core.delete_experiment_capture_files(pane_id).unwrap_err();
        assert_eq!(errors::kind(&busy), soyeht_protocol::ErrorKind::PaneBusy);
        drop(core.terminal_captures.remove(pane_id));

        let exports = core.observation_root().unwrap().join("exports");
        fs::create_dir(&exports).unwrap();
        fs::set_permissions(&exports, fs::Permissions::from_mode(0o700)).unwrap();
        let export = exports.join("export-preserved.jsonl");
        fs::write(&export, b"raw terminal export\n").unwrap();
        fs::set_permissions(&export, fs::Permissions::from_mode(0o600)).unwrap();

        let (sender, _receiver) = outbox::channel();
        let connection = core.connect(true, true, sender).unwrap();
        assert!(matches!(
            core.dispatch(
                &connection,
                serde_json::json!({
                    "jsonrpc":"2.0","id":"hello","method":"hello",
                    "params":{"client":"delete-test","clientVersion":"1","protocolVersion":1}
                }),
            )
            .unwrap(),
            Outcome::Reply(_)
        ));
        let Outcome::Reply(result) = core
            .dispatch(
                &connection,
                serde_json::json!({
                    "jsonrpc":"2.0","id":"delete","method":"pane.experiment.delete",
                    "params":{"paneId":pane_id,"idempotencyKey":"delete-once"}
                }),
            )
            .unwrap()
        else {
            panic!("expected deletion reply")
        };
        assert!(result["files"].as_u64().unwrap() > 0);
        assert_ne!(result["bytes"], "0");
        assert_eq!(result["nothingRemained"], true);
        assert_eq!(result["exportsPath"], exports.to_str().unwrap());
        assert_eq!(fs::read(&export).unwrap(), b"raw terminal export\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deletion_commit_failure_states_that_the_data_was_already_removed() {
        let error = deletion_receipt_commit_error(
            &DeletionReceipt {
                files: 3,
                bytes: 144,
            },
            "/profile/experiments/pane-state/exports",
            errors::internal("receipt store unavailable"),
        );
        assert_eq!(
            error.to_string(),
            "Captured experiment data was removed (3 files, 144 bytes), but the record of its removal was not saved: receipt store unavailable. Export files were not deleted and remain in /profile/experiments/pane-state/exports"
        );
    }

    #[test]
    fn projection_worker_records_byte_gap_then_continues_in_a_fresh_epoch() {
        let directory = std::env::temp_dir().join(format!("sn-byte-gap-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let build = EngineBuild {
            package_version: "0.1.0".into(),
            executable_sha256: "a".repeat(64),
        };
        let store = SegmentStore::open(
            &directory,
            "pane-1",
            ObservationPaneKind::Terminal,
            HarnessIdentity::Unknown,
            build.clone(),
        )
        .unwrap();
        let recorder = ObservationRecorder::open(store).unwrap();
        let projection = TerminalProjection::new(
            "pane-1",
            ObservationPaneKind::Terminal,
            HarnessIdentity::Unknown,
            4,
            20,
            0,
        )
        .unwrap();
        let (sender, receiver) = channel::bounded(1);
        sender
            .send(ProjectionCommand::Bytes {
                timestamp_ms: 1,
                bytes: b"before".to_vec().into(),
                counted: false,
            })
            .unwrap();
        let overflow = Arc::new(Mutex::new(Some(RawByteGap {
            first_timestamp_ms: 2,
            last_timestamp_ms: 3,
            dropped_bytes: 4_200_000,
        })));
        let worker_overflow = Arc::clone(&overflow);
        let worker_overflow_active = Arc::new(AtomicBool::new(true));
        let worker = thread::spawn(move || {
            projection_loop(
                projection,
                recorder,
                receiver,
                worker_overflow,
                worker_overflow_active,
                Arc::new(TestQueueMetrics::default()),
                Arc::new(TestProjectionGate::default()),
            )
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while overflow.lock().unwrap().is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "projection worker did not persist the byte gap"
            );
            thread::sleep(Duration::from_millis(5));
        }
        sender
            .send(ProjectionCommand::Bytes {
                timestamp_ms: 4,
                bytes: b"after".to_vec().into(),
                counted: false,
            })
            .unwrap();
        drop(sender);
        worker.join().unwrap().unwrap();

        let rows = SegmentStore::open(
            &directory,
            "pane-1",
            ObservationPaneKind::Terminal,
            HarnessIdentity::Unknown,
            build,
        )
        .unwrap()
        .replay()
        .unwrap()
        .into_iter()
        .flat_map(|segment| segment.rows)
        .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        let RecordedRow::Observation(before) = &rows[0] else {
            panic!("first projected frame was not recorded")
        };
        assert!(before.lines.iter().any(|line| line.contains("before")));
        let RecordedRow::Gap(gap) = &rows[1] else {
            panic!("raw handoff loss was not recorded as a gap")
        };
        assert_eq!(gap.dropped_observations, None);
        assert_eq!(gap.dropped_bytes, Some(4_200_000));
        let RecordedRow::Observation(after) = &rows[2] else {
            panic!("capture did not continue after the byte gap")
        };
        assert!(after.lines.iter().any(|line| line.contains("after")));
        assert!(after.lines.iter().all(|line| !line.contains("before")));
        fs::remove_dir_all(directory).unwrap();
    }

    fn noisy_chunks() -> Vec<Vec<u8>> {
        (0..CADENCE_RUNS * CADENCE_POLLS_PER_SAMPLE + 2)
            .map(|run| {
                let mut bytes = format!("\x1b]2;build-{run}\x07\x1b[2J\x1b[H").into_bytes();
                for line in 0..800 {
                    bytes.extend_from_slice(
                        format!(
                            "compile unit {run:02}-{line:04} ................................\r\n"
                        )
                        .as_bytes(),
                    );
                }
                bytes
            })
            .collect()
    }

    fn capture_metrics(core: &Core) -> Arc<TestQueueMetrics> {
        Arc::clone(
            &core
                .terminal_captures
                .get("cadence-pane")
                .unwrap()
                .test_queue_metrics,
        )
    }

    fn wait_for_projection_worker(metrics: &TestQueueMetrics) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if metrics
                .outstanding
                .load(std::sync::atomic::Ordering::Relaxed)
                == 0
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "terminal projection worker did not drain"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait_for_processed(metrics: &TestQueueMetrics, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if metrics.processed.load(Ordering::Acquire) >= expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "terminal projection worker did not process the measured sample"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn set_cadence_projection_paused(core: &Core, paused: bool) {
        core.terminal_captures["cadence-pane"]
            .test_projection_gate
            .set_paused(paused);
    }

    fn peak_outstanding(metrics: &TestQueueMetrics) -> usize {
        metrics
            .peak_outstanding
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn replay_cadence_rows(directory: &Path, build: EngineBuild) -> Vec<RecordedRow> {
        SegmentStore::open(
            &directory.join("on/experiments/pane-state"),
            "cadence-pane",
            ObservationPaneKind::Terminal,
            HarnessIdentity::Unknown,
            build,
        )
        .unwrap()
        .replay()
        .unwrap()
        .into_iter()
        .flat_map(|segment| segment.rows)
        .collect()
    }

    fn cadence_core(root: &Path, chunks: &[Vec<u8>], capture: bool) -> Core {
        fs::create_dir(root).unwrap();
        let mut core = Core::open(root).unwrap();
        // This benchmark measures poll_output and the capture handoff. Keep
        // unrelated local-server discovery deterministic across retries.
        core.clock = Arc::new(FixedClock(0));
        core.web = crate::web::Service::new(Box::new(EmptyPortScan));
        let pane_id = "cadence-pane".to_owned();
        let pane = Pane {
            pane_id: pane_id.clone(),
            workspace_id: "workspace".into(),
            name: "Cadence".into(),
            cwd: root.display().to_string(),
            mode: PaneMode::Terminal,
            state: PaneState::Unknown,
            source: StateSource::None,
            confidence: 0.0,
            phrase: "Shell active".into(),
            generation: 1,
            cols: 120,
            rows: 40,
            record: false,
            experiment_capture: Some(PaneExperimentCapture {
                on: capture,
                failure: None,
            }),
            pid: Some(1),
            stream_id: Some("cadence-stream".into()),
            agent: None,
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            web_session: None,
            writing_kind: None,
            writing_section: None,
        };
        let screen = Screen::open(&root.join("sessions").join(&pane_id), false).unwrap();
        core.panes.insert(
            pane_id.clone(),
            RunningPane {
                stored: StoredPane {
                    pane,
                    closed: false,
                    instance_key: "cadence-key".into(),
                    instance_id: Some("cadence-instance".into()),
                    supervisor_id: Some("cadence-supervisor".into()),
                    sequence_base: 0,
                },
                screen,
            },
        );
        let mut responses = VecDeque::new();
        let mut offset = 0_u64;
        for chunk in chunks {
            let next = offset + chunk.len() as u64;
            responses.push_back(Ok(Response::Output(Output {
                from_seq: offset,
                next_seq: next,
                end_seq: next,
                bytes: STANDARD.encode(chunk),
                gap: None,
                exited: false,
            })));
            responses.push_back(Ok(Response::Ok));
            offset = next;
        }
        core.ptyd = Some(Box::new(FakeSupervisor {
            responses,
            id: "cadence-supervisor".into(),
            generation: core.generation as u32,
            ..FakeSupervisor::default()
        }));
        if capture {
            let runtime = core.prepare_terminal_capture(&pane_id).unwrap();
            core.terminal_captures.insert(pane_id, runtime);
        }
        core
    }

    fn percentile(values: &[u128], numerator: usize, denominator: usize) -> u128 {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let index = (sorted.len() * numerator).div_ceil(denominator) - 1;
        sorted[index]
    }

    fn capture_is_not_faster(off_micros: &[u128], on_micros: &[u128]) -> bool {
        percentile(on_micros, 1, 2) >= percentile(off_micros, 1, 2)
            && percentile(on_micros, 95, 100) >= percentile(off_micros, 95, 100)
    }

    #[test]
    fn cadence_sanity_rejects_an_impossibly_faster_capture_run() {
        assert!(!capture_is_not_faster(
            &[331, 333, 345, 352],
            &[329, 330, 341, 350]
        ));
        assert!(capture_is_not_faster(
            &[321, 323, 327, 331],
            &[326, 329, 333, 353]
        ));
    }

    #[test]
    #[ignore = "measured separately by scripts/check_all.sh without competing tests"]
    fn frame_dense_worker_headroom_reports_queue_pressure_and_time_coverage() {
        let directory = std::env::temp_dir().join(format!("sn-headroom-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let build = EngineBuild {
            package_version: "0.1.0".into(),
            executable_sha256: "b".repeat(64),
        };
        let capture = TerminalCapture::open(
            &directory,
            "frame-dense-pane",
            HarnessIdentity::Unknown,
            40,
            120,
            build.clone(),
        )
        .unwrap();
        let metrics = Arc::clone(&capture.test_queue_metrics);
        let started = Instant::now();
        let mut submitted_bytes = 0_usize;
        for index in 0..FRAME_DENSE_OBSERVATIONS {
            let target = started + FRAME_DENSE_INTERVAL * u32::try_from(index).unwrap();
            if let Some(remaining) = target.checked_duration_since(Instant::now()) {
                thread::sleep(remaining);
            }
            let bytes = format!("\rcompile frame {index:04}").into_bytes();
            submitted_bytes += bytes.len();
            capture
                .record(
                    i64::try_from(started.elapsed().as_millis()).unwrap(),
                    bytes.into(),
                )
                .unwrap();
        }
        let submission_wall_ms = started.elapsed().as_millis();
        drop(capture);
        let peak_outstanding = peak_outstanding(&metrics);
        let rows = SegmentStore::open(
            &directory,
            "frame-dense-pane",
            ObservationPaneKind::Terminal,
            HarnessIdentity::Unknown,
            build,
        )
        .unwrap()
        .replay()
        .unwrap()
        .into_iter()
        .flat_map(|segment| segment.rows)
        .collect::<Vec<_>>();
        let observations = rows
            .iter()
            .filter(|row| matches!(row, RecordedRow::Observation(_)))
            .count();
        let gaps = rows
            .iter()
            .filter_map(|row| match row {
                RecordedRow::Gap(gap) => Some(gap),
                _ => None,
            })
            .collect::<Vec<_>>();
        let dropped_observations = gaps
            .iter()
            .filter_map(|gap| gap.dropped_observations)
            .sum::<u64>();
        let dropped_bytes = gaps.iter().filter_map(|gap| gap.dropped_bytes).sum::<u64>();
        let row_refs = rows.iter().collect::<Vec<_>>();
        let coverage_basis_points = recorded_time_coverage_basis_points(&row_refs);
        eprintln!(
            "capture headroom submitted_bytes={submitted_bytes} submission_wall_ms={submission_wall_ms} observations={observations} gaps={} dropped_observations={dropped_observations} dropped_bytes={dropped_bytes} peak_outstanding={peak_outstanding} time_coverage_basis_points={coverage_basis_points}",
            gaps.len()
        );
        assert!(!rows.is_empty());
        assert!(peak_outstanding <= PROJECTION_QUEUE_CAPACITY + 1);
        assert!(
            coverage_basis_points >= 9_500,
            "frame-dense capture fell below the predeclared 95 percent coverage floor"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    fn run_cadence_attempt(directory: &Path, chunks: &[Vec<u8>], attempt: usize) -> bool {
        fs::create_dir(directory).unwrap();
        let load_average_before = load_average();
        let fixture_bytes = chunks.iter().skip(2).map(Vec::len).sum::<usize>();
        assert_eq!(fixture_bytes, CADENCE_FIXTURE_BYTES);
        let mut off = cadence_core(&directory.join("off"), chunks, false);
        let mut on = cadence_core(&directory.join("on"), chunks, false);
        off.poll_output().unwrap();
        on.poll_output().unwrap();
        let runtime = on.prepare_terminal_capture("cadence-pane").unwrap();
        on.terminal_captures.insert("cadence-pane".into(), runtime);
        let metrics = capture_metrics(&on);
        off.poll_output().unwrap();
        on.poll_output().unwrap();
        wait_for_projection_worker(&metrics);
        metrics.enabled.store(false, Ordering::Relaxed);
        thread::sleep(CADENCE_WARMUP_DRAIN);
        set_cadence_projection_paused(&on, true);
        let mut off_micros = Vec::new();
        let mut on_micros = Vec::new();
        let mut expected_processed = metrics.processed.load(Ordering::Acquire);
        let fixture_started = Instant::now();
        for pair in 0..CADENCE_RUNS {
            let (first, second) = if pair % 2 == 0 {
                (&mut off, &mut on)
            } else {
                (&mut on, &mut off)
            };
            let mut first_micros = 0_u128;
            let mut second_micros = 0_u128;
            for _ in 0..CADENCE_POLLS_PER_SAMPLE {
                let started = Instant::now();
                first.poll_output().unwrap();
                first_micros += started.elapsed().as_micros();
                let started = Instant::now();
                second.poll_output().unwrap();
                second_micros += started.elapsed().as_micros();
            }
            if pair % 2 == 0 {
                off_micros.push(first_micros);
                on_micros.push(second_micros);
            } else {
                on_micros.push(first_micros);
                off_micros.push(second_micros);
            }
            // Drain outside the timed sample so each pair starts with the
            // same empty bounded queue and never measures saturation.
            set_cadence_projection_paused(&on, false);
            expected_processed += CADENCE_POLLS_PER_SAMPLE;
            wait_for_processed(&metrics, expected_processed);
            set_cadence_projection_paused(&on, true);
        }
        set_cadence_projection_paused(&on, false);
        wait_for_projection_worker(&metrics);
        if !cadence_samples_pass(attempt, load_average_before, &off_micros, &on_micros) {
            drop(on);
            drop(off);
            fs::remove_dir_all(directory).unwrap();
            return false;
        }
        let build = on.experiment_capture_build.clone().unwrap();
        drop(on);
        let fixture_wall_micros = fixture_started.elapsed().as_micros();
        drop(off);
        let rows = replay_cadence_rows(directory, build);
        let observations = rows
            .iter()
            .filter(|row| matches!(row, RecordedRow::Observation(_)))
            .count();
        let gaps = rows
            .iter()
            .filter(|row| matches!(row, RecordedRow::Gap(_)))
            .count();
        let dropped_observations = rows
            .iter()
            .filter_map(|row| match row {
                RecordedRow::Gap(gap) => gap.dropped_observations,
                _ => None,
            })
            .sum::<u64>();
        let dropped_bytes = rows
            .iter()
            .filter_map(|row| match row {
                RecordedRow::Gap(gap) => gap.dropped_bytes,
                _ => None,
            })
            .sum::<u64>();
        let row_refs = rows.iter().collect::<Vec<_>>();
        let coverage_basis_points = recorded_time_coverage_basis_points(&row_refs);
        eprintln!(
            "capture fixture bytes={fixture_bytes} wall_us={fixture_wall_micros} expected_measured_observations={}",
            CADENCE_RUNS * CADENCE_POLLS_PER_SAMPLE
        );
        eprintln!(
            "capture coverage observations={observations} gaps={gaps} dropped_observations={dropped_observations} dropped_bytes={dropped_bytes} time_coverage_basis_points={coverage_basis_points}"
        );
        assert_complete_cadence_coverage(observations, gaps, coverage_basis_points);
        assert_eq!(
            chunks
                .iter()
                .skip(2 + CADENCE_WARMUP_PAIRS * CADENCE_POLLS_PER_SAMPLE)
                .map(Vec::len)
                .sum::<usize>(),
            CADENCE_STEADY_BYTES
        );
        fs::remove_dir_all(directory).unwrap();
        true
    }

    #[test]
    #[ignore = "measured separately by scripts/check_all.sh without competing tests"]
    fn capture_cadence_measurement_uses_ten_paired_noisy_samples() {
        let directory = std::env::temp_dir().join(format!("sn-cadence-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let chunks = noisy_chunks();
        for attempt in 1..=CADENCE_MAX_ATTEMPTS {
            if run_cadence_attempt(
                &directory.join(format!("attempt-{attempt}")),
                &chunks,
                attempt,
            ) {
                fs::remove_dir_all(directory).unwrap();
                return;
            }
        }
        fs::remove_dir_all(directory).unwrap();
        panic!("cadence did not produce a steady sample in {CADENCE_MAX_ATTEMPTS} attempts");
    }
}
