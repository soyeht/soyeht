use crate::{
    core::Core,
    errors::{self, Result},
    pane_observations::{
        ObservationRecorder, SegmentStore, experiment_root, installed_engine_build,
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
        let worker = thread::Builder::new()
            .name("terminal-observation-projector".into())
            .spawn(move || {
                if let Err(error) = projection_loop(projection, recorder, receiver, worker_overflow)
                    && let Ok(mut slot) = worker_failure.lock()
                {
                    *slot = Some(error.to_string());
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            overflow,
            failure,
            worker: Some(worker),
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
        match sender.try_send(ProjectionCommand::Bytes {
            timestamp_ms,
            bytes,
        }) {
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
) -> Result<()> {
    loop {
        match receiver.recv_timeout(FAILURE_CHECK_INTERVAL) {
            Ok(ProjectionCommand::Bytes {
                timestamp_ms,
                bytes,
            }) => {
                if let Some(frame) = projection.process(timestamp_ms, &bytes)? {
                    recorder.try_record(frame)?;
                }
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

    #[test]
    fn full_projection_handoff_coalesces_exact_bytes_without_failing_capture() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let overflow = Arc::new(Mutex::new(None));
        let capture = TerminalCapture {
            sender: Some(sender),
            overflow: Arc::clone(&overflow),
            failure: Arc::new(Mutex::new(None)),
            worker: None,
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
        let worker =
            thread::spawn(move || projection_loop(projection, recorder, receiver, worker_overflow));

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
        (0..CADENCE_RUNS)
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

    #[test]
    fn capture_cadence_measurement_uses_ten_paired_noisy_poll_cycles() {
        let directory = std::env::temp_dir().join(format!("sn-cadence-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let chunks = noisy_chunks();
        let mut off = cadence_core(&directory.join("off"), &chunks, false);
        let mut on = cadence_core(&directory.join("on"), &chunks, true);
        let mut off_micros = Vec::new();
        let mut on_micros = Vec::new();
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
        let off_median = percentile(&off_micros, 1, 2);
        let on_median = percentile(&on_micros, 1, 2);
        let off_p95 = percentile(&off_micros, 95, 100);
        let on_p95 = percentile(&on_micros, 95, 100);
        eprintln!("capture cadence off_us={off_micros:?} on_us={on_micros:?}");
        assert!(on_median * 100 <= off_median * 105);
        assert!(on_p95 <= off_p95 + 5_000);
        let build = on.terminal_capture_build.clone().unwrap();
        drop(on);
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
            "capture coverage observations={observations} gaps={gaps} dropped_observations={dropped_observations} dropped_bytes={dropped_bytes} time_coverage_percent=100"
        );
        assert!(observations > 0);
        assert_eq!(gaps, 0);
        fs::remove_dir_all(directory).unwrap();
    }
}
