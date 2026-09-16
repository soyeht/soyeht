use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{
        DeletionReceipt, ObservationRecorder, SegmentStore, delete_pane, experiment_root,
        installed_engine_build,
    },
    terminal_projection::TerminalProjection,
};
use soyeht_pane_state::{EngineBuild, HarnessIdentity};
use soyeht_protocol::{PaneExperimentCapture, PaneMode};
use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

const CAPTURE_FAILURE: &str = "Experiment capture stopped because its private recorder failed";
const PROJECTION_QUEUE_CAPACITY: usize = 32;
const FAILURE_CHECK_INTERVAL: Duration = Duration::from_millis(50);

#[cfg(test)]
#[derive(Default)]
struct TestQueueMetrics {
    outstanding: usize,
    peak_outstanding: usize,
}

enum ProjectionCommand {
    Bytes { timestamp_ms: i64, bytes: Vec<u8> },
    Resize { rows: u16, cols: u16 },
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
    sender: Option<mpsc::SyncSender<ProjectionCommand>>,
    overflow: Arc<Mutex<Option<RawByteGap>>>,
    failure: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
    #[cfg(test)]
    test_queue_metrics: Arc<Mutex<TestQueueMetrics>>,
}

impl TerminalCapture {
    fn open(root: &Path, pane_id: &str, rows: u32, cols: u32, build: EngineBuild) -> Result<Self> {
        let rows = u16::try_from(rows)
            .map_err(|_| errors::invalid("Pane rows exceed the observation range"))?;
        let cols = u16::try_from(cols)
            .map_err(|_| errors::invalid("Pane columns exceed the observation range"))?;
        let store = SegmentStore::open(root, pane_id, HarnessIdentity::Unknown, build)?;
        let sequence = store.next_sequence()?;
        let projection = TerminalProjection::new(pane_id, rows, cols, sequence)?;
        let recorder = ObservationRecorder::open(store)?;
        let (sender, receiver) = mpsc::sync_channel(PROJECTION_QUEUE_CAPACITY);
        let overflow = Arc::new(Mutex::new(None));
        let failure = Arc::new(Mutex::new(None));
        let worker_overflow = Arc::clone(&overflow);
        let worker_failure = Arc::clone(&failure);
        #[cfg(test)]
        let test_queue_metrics = Arc::new(Mutex::new(TestQueueMetrics::default()));
        #[cfg(test)]
        let worker_test_queue_metrics = Arc::clone(&test_queue_metrics);
        let worker = thread::Builder::new()
            .name("terminal-observation-projector".into())
            .spawn(move || {
                if let Err(error) = projection_loop(
                    projection,
                    recorder,
                    receiver,
                    worker_overflow,
                    #[cfg(test)]
                    worker_test_queue_metrics,
                ) && let Ok(mut slot) = worker_failure.lock()
                {
                    *slot = Some(error.to_string());
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            overflow,
            failure,
            worker: Some(worker),
            #[cfg(test)]
            test_queue_metrics,
        })
    }

    fn record(&self, timestamp_ms: i64, bytes: Vec<u8>) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let mut overflow = self
            .overflow
            .lock()
            .map_err(|_| errors::internal("Terminal projection gap lock was poisoned"))?;
        if let Some(gap) = overflow.as_mut() {
            return gap.extend(timestamp_ms, bytes.len());
        }
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| errors::internal("Terminal projection worker is closed"))?;
        #[cfg(test)]
        let mut metrics = self.test_queue_metrics.lock().unwrap();
        let outcome = sender.try_send(ProjectionCommand::Bytes {
            timestamp_ms,
            bytes,
        });
        #[cfg(test)]
        if outcome.is_ok() {
            metrics.outstanding += 1;
            metrics.peak_outstanding = metrics.peak_outstanding.max(metrics.outstanding);
        }
        #[cfg(test)]
        drop(metrics);
        match outcome {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(ProjectionCommand::Bytes {
                timestamp_ms,
                bytes,
            })) => {
                *overflow = Some(RawByteGap::new(timestamp_ms, bytes.len())?);
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                Err(errors::internal("Terminal projection worker stopped"))
            }
            Err(mpsc::TrySendError::Full(ProjectionCommand::Resize { .. })) => {
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

    fn failed(&self) -> bool {
        match self.failure.lock() {
            Ok(failure) => failure.is_some(),
            Err(_) => true,
        }
    }
}

impl Drop for TerminalCapture {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn projection_loop(
    mut projection: TerminalProjection,
    recorder: ObservationRecorder,
    receiver: mpsc::Receiver<ProjectionCommand>,
    overflow: Arc<Mutex<Option<RawByteGap>>>,
    #[cfg(test)] test_queue_metrics: Arc<Mutex<TestQueueMetrics>>,
) -> Result<()> {
    loop {
        match receiver.recv_timeout(FAILURE_CHECK_INTERVAL) {
            Ok(ProjectionCommand::Bytes {
                timestamp_ms,
                bytes,
            }) => {
                let result: Result<()> = (|| {
                    if let Some(frame) = projection.process(timestamp_ms, &bytes)? {
                        recorder.try_record(frame)?;
                    }
                    Ok(())
                })();
                #[cfg(test)]
                {
                    let mut metrics = test_queue_metrics.lock().unwrap();
                    metrics.outstanding = metrics.outstanding.saturating_sub(1);
                }
                result?;
            }
            Ok(ProjectionCommand::Resize { rows, cols }) => projection.resize(rows, cols)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(gap) = take_raw_gap(&overflow)? {
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
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(gap) = take_raw_gap(&overflow)? {
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

fn take_raw_gap(overflow: &Mutex<Option<RawByteGap>>) -> Result<Option<RawByteGap>> {
    Ok(overflow
        .lock()
        .map_err(|_| errors::internal("Terminal projection gap lock was poisoned"))?
        .take())
}

impl Core {
    fn observation_root(&self) -> Result<std::path::PathBuf> {
        experiment_root(&self.directory)
    }

    fn capture_build(&mut self) -> Result<EngineBuild> {
        if let Some(build) = &self.terminal_capture_build {
            return Ok(build.clone());
        }
        let build = installed_engine_build()?;
        self.terminal_capture_build = Some(build.clone());
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
            stored.pane.rows,
            stored.pane.cols,
            build,
        )
    }

    pub(crate) fn delete_terminal_capture_data(
        &mut self,
        connection: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let pane_id = errors::param_str(params, "paneId")?;
        let receipt = self.delete_terminal_capture_files(pane_id)?;
        let files = u32::try_from(receipt.files)
            .map_err(|_| errors::internal("Observation deletion file count is invalid"))?;
        let result = serde_json::json!({"files":files,"bytes":receipt.bytes.to_string()});
        Ok(self
            .mutate(connection, "pane.experiment.delete", params, |_tx| {
                Ok((result, vec![]))
            })?
            .result)
    }

    fn delete_terminal_capture_files(&mut self, pane_id: &str) -> Result<DeletionReceipt> {
        let pane = self
            .panes
            .get(pane_id)
            .filter(|pane| !pane.stored.closed && pane.stored.pane.mode == PaneMode::Terminal)
            .ok_or_else(|| errors::not_found("Terminal pane not found"))?;
        if pane
            .stored
            .pane
            .experiment_capture
            .as_ref()
            .is_some_and(|capture| capture.on)
            || self.terminal_captures.contains_key(pane_id)
        {
            return Err(errors::pane_busy(
                "Stop experiment capture before deleting its data",
            ));
        }
        delete_pane(&self.observation_root()?, pane_id)
    }

    pub(crate) fn restore_terminal_captures(&mut self) -> Result<()> {
        let ids: Vec<_> = self
            .panes
            .iter()
            .filter(|(_, pane)| {
                !pane.stored.closed
                    && pane.stored.pane.mode == PaneMode::Terminal
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
            match self.prepare_terminal_capture(&id) {
                Ok(capture) => {
                    self.terminal_captures.insert(id, capture);
                }
                Err(_) => self.fail_terminal_capture(&id)?,
            }
        }
        Ok(())
    }

    pub(crate) fn observe_terminal(&mut self, pane_id: &str, bytes: Vec<u8>) -> Result<()> {
        let timestamp_ms = self.now_ms();
        let failed = self
            .terminal_captures
            .get_mut(pane_id)
            .is_some_and(|capture| capture.record(timestamp_ms, bytes).is_err());
        if failed {
            self.fail_terminal_capture(pane_id)?;
        }
        Ok(())
    }

    pub(crate) fn reconcile_terminal_captures(&mut self) -> Result<()> {
        let failed: Vec<_> = self
            .terminal_captures
            .iter()
            .filter(|(_, capture)| capture.failed())
            .map(|(pane_id, _)| pane_id.clone())
            .collect();
        for pane_id in failed {
            self.fail_terminal_capture(&pane_id)?;
        }
        Ok(())
    }

    pub(crate) fn resize_terminal_capture(&mut self, pane_id: &str) -> Result<()> {
        let Some(capture) = self.terminal_captures.get_mut(pane_id) else {
            return Ok(());
        };
        let pane = &self.panes[pane_id].stored.pane;
        if capture.resize(pane.rows, pane.cols).is_err() {
            self.fail_terminal_capture(pane_id)?;
        }
        Ok(())
    }

    pub(crate) fn fail_terminal_capture(&mut self, pane_id: &str) -> Result<()> {
        self.terminal_captures.remove(pane_id);
        let mut saved = errors::invariant(
            self.panes.get(pane_id),
            "Captured pane disappeared before failure publication",
        )?
        .stored
        .clone();
        saved.pane.experiment_capture = Some(PaneExperimentCapture {
            on: false,
            failure: Some(CAPTURE_FAILURE.into()),
        });
        self.save_runtime(pane_id, saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{RunningPane, StoredPane},
        ports::testing::FakeSupervisor,
        screen::Screen,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use soyeht_pane_state::RecordedRow;
    use soyeht_protocol::{Pane, PaneState, StateSource};
    use soyeht_ptyd::wire::{Output, Response};
    use std::{collections::VecDeque, fs, os::unix::fs::PermissionsExt, time::Instant};
    use uuid::Uuid;

    const CADENCE_RUNS: usize = 10;
    const CADENCE_MAX_ATTEMPTS: usize = 10;
    const CADENCE_WARMUP_PAIRS: usize = 3;
    const CADENCE_FIXTURE_BYTES: usize = 440_192;
    const CADENCE_STEADY_BYTES: usize = 308_135;
    const CADENCE_WARMUP_DRAIN: Duration = Duration::from_millis(60);
    const FRAME_DENSE_INTERVAL: Duration = Duration::from_millis(5);
    const FRAME_DENSE_OBSERVATIONS: usize = 400;

    #[test]
    fn full_projection_handoff_coalesces_exact_bytes_without_failing_capture() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let overflow = Arc::new(Mutex::new(None));
        let capture = TerminalCapture {
            sender: Some(sender),
            overflow: Arc::clone(&overflow),
            failure: Arc::new(Mutex::new(None)),
            worker: None,
            test_queue_metrics: Arc::new(Mutex::new(TestQueueMetrics::default())),
        };
        capture.record(10, vec![b'a']).unwrap();
        capture.record(20, vec![b'b'; 4]).unwrap();
        capture.record(30, vec![b'c'; 7]).unwrap();

        let gap = overflow.lock().unwrap().clone().unwrap();
        assert_eq!(gap.first_timestamp_ms, 20);
        assert_eq!(gap.last_timestamp_ms, 30);
        assert_eq!(gap.dropped_bytes, 11);
        assert!(!capture.failed());
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
    fn pane_capture_data_deletes_only_after_its_worker_stops_and_returns_a_receipt() {
        let directory = std::env::temp_dir().join(format!("sn-delete-capture-{}", Uuid::new_v4()));
        let mut core = cadence_core(&directory, &[], false);
        let pane_id = "cadence-pane";
        let capture = core.prepare_terminal_capture(pane_id).unwrap();
        capture
            .record(1, b"\x1b[2J\x1b[Hcaptured terminal text\r\n".to_vec())
            .unwrap();
        drop(capture);

        let active = core.prepare_terminal_capture(pane_id).unwrap();
        core.terminal_captures.insert(pane_id.into(), active);
        let busy = core.delete_terminal_capture_files(pane_id).unwrap_err();
        assert_eq!(errors::kind(&busy), soyeht_protocol::ErrorKind::PaneBusy);
        drop(core.terminal_captures.remove(pane_id));

        let receipt = core.delete_terminal_capture_files(pane_id).unwrap();
        assert!(receipt.files > 0);
        assert!(receipt.bytes > 0);
        assert_eq!(
            core.delete_terminal_capture_files(pane_id).unwrap().files,
            0
        );
        fs::remove_dir_all(directory).unwrap();
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
            HarnessIdentity::Unknown,
            build.clone(),
        )
        .unwrap();
        let recorder = ObservationRecorder::open(store).unwrap();
        let projection = TerminalProjection::new("pane-1", 4, 20, 0).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(ProjectionCommand::Bytes {
                timestamp_ms: 1,
                bytes: b"before".to_vec(),
            })
            .unwrap();
        let overflow = Arc::new(Mutex::new(Some(RawByteGap {
            first_timestamp_ms: 2,
            last_timestamp_ms: 3,
            dropped_bytes: 4_200_000,
        })));
        let worker_overflow = Arc::clone(&overflow);
        let worker = thread::spawn(move || {
            projection_loop(
                projection,
                recorder,
                receiver,
                worker_overflow,
                Arc::new(Mutex::new(TestQueueMetrics::default())),
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
                bytes: b"after".to_vec(),
            })
            .unwrap();
        drop(sender);
        worker.join().unwrap().unwrap();

        let rows = SegmentStore::open(&directory, "pane-1", HarnessIdentity::Unknown, build)
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
        (0..CADENCE_RUNS + 2)
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

    fn capture_metrics(core: &Core) -> Arc<Mutex<TestQueueMetrics>> {
        Arc::clone(
            &core
                .terminal_captures
                .get("cadence-pane")
                .unwrap()
                .test_queue_metrics,
        )
    }

    fn wait_for_projection_worker(metrics: &Mutex<TestQueueMetrics>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if metrics.lock().unwrap().outstanding == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "terminal projection worker did not drain"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn cadence_core(root: &Path, chunks: &[Vec<u8>], capture: bool) -> Core {
        fs::create_dir(root).unwrap();
        let mut core = Core::open(root).unwrap();
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
    fn frame_dense_worker_headroom_reports_queue_pressure_and_time_coverage() {
        let directory = std::env::temp_dir().join(format!("sn-headroom-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let build = EngineBuild {
            package_version: "0.1.0".into(),
            executable_sha256: "b".repeat(64),
        };
        let capture =
            TerminalCapture::open(&directory, "frame-dense-pane", 40, 120, build.clone()).unwrap();
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
                .record(i64::try_from(started.elapsed().as_millis()).unwrap(), bytes)
                .unwrap();
        }
        let submission_wall_ms = started.elapsed().as_millis();
        drop(capture);
        let peak_outstanding = metrics.lock().unwrap().peak_outstanding;
        let rows = SegmentStore::open(
            &directory,
            "frame-dense-pane",
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
        let session_ms = u64::try_from(last_timestamp.saturating_sub(first_timestamp)).unwrap();
        let gap_ms = gaps.iter().fold(0_u64, |total, gap| {
            total.saturating_add(
                u64::try_from(gap.last_timestamp_ms.saturating_sub(gap.first_timestamp_ms))
                    .unwrap(),
            )
        });
        let coverage_basis_points = session_ms
            .saturating_sub(gap_ms)
            .saturating_mul(10_000)
            .checked_div(session_ms)
            .unwrap_or(10_000);
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
        thread::sleep(CADENCE_WARMUP_DRAIN);
        {
            let mut metrics = metrics.lock().unwrap();
            metrics.peak_outstanding = metrics.outstanding;
        }
        let mut off_micros = Vec::new();
        let mut on_micros = Vec::new();
        let fixture_started = Instant::now();
        for pair in 0..CADENCE_RUNS {
            let (first, second) = if pair % 2 == 0 {
                (&mut off, &mut on)
            } else {
                (&mut on, &mut off)
            };
            let started = Instant::now();
            first.poll_output().unwrap();
            let first_micros = started.elapsed().as_micros();
            let started = Instant::now();
            second.poll_output().unwrap();
            let second_micros = started.elapsed().as_micros();
            if pair % 2 == 0 {
                off_micros.push(first_micros);
                on_micros.push(second_micros);
            } else {
                on_micros.push(first_micros);
                off_micros.push(second_micros);
            }
        }
        wait_for_projection_worker(&metrics);
        let peak_outstanding = metrics.lock().unwrap().peak_outstanding;
        let steady_off = &off_micros[CADENCE_WARMUP_PAIRS..];
        let steady_on = &on_micros[CADENCE_WARMUP_PAIRS..];
        let off_median = percentile(steady_off, 1, 2);
        let on_median = percentile(steady_on, 1, 2);
        let off_p95 = percentile(steady_off, 95, 100);
        let on_p95 = percentile(steady_on, 95, 100);
        eprintln!("capture cadence attempt={attempt} off_us={off_micros:?} on_us={on_micros:?}");
        if !capture_is_not_faster(steady_off, steady_on) {
            eprintln!(
                "capture cadence attempt={attempt} invalid: capture-on measured faster than capture-off after the declared warm-up"
            );
            drop(on);
            drop(off);
            fs::remove_dir_all(directory).unwrap();
            return false;
        }
        assert!(on_median * 100 <= off_median * 105);
        assert!(on_p95 <= off_p95 + 5_000);
        let build = on.terminal_capture_build.clone().unwrap();
        drop(on);
        let fixture_wall_micros = fixture_started.elapsed().as_micros();
        drop(off);
        let replayed = SegmentStore::open(
            &directory.join("on/experiments/pane-state"),
            "cadence-pane",
            HarnessIdentity::Unknown,
            build,
        )
        .unwrap()
        .replay()
        .unwrap();
        let rows = replayed
            .iter()
            .flat_map(|segment| segment.rows.iter())
            .collect::<Vec<_>>();
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
        eprintln!(
            "capture fixture bytes={fixture_bytes} wall_us={fixture_wall_micros} expected_measured_observations={CADENCE_RUNS} peak_outstanding={peak_outstanding}"
        );
        eprintln!(
            "capture coverage observations={observations} gaps={gaps} dropped_observations={dropped_observations} dropped_bytes={dropped_bytes} time_coverage_percent=100"
        );
        assert_eq!(observations, CADENCE_RUNS + 1);
        assert_eq!(gaps, 0);
        assert_eq!(
            chunks
                .iter()
                .skip(2 + CADENCE_WARMUP_PAIRS)
                .map(Vec::len)
                .sum::<usize>(),
            CADENCE_STEADY_BYTES
        );
        fs::remove_dir_all(directory).unwrap();
        true
    }

    #[test]
    fn capture_cadence_measurement_uses_ten_paired_noisy_poll_cycles() {
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
