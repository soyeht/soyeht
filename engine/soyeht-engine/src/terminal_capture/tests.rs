mod cadence_stats;

use cadence_stats::{
    CADENCE_MEDIAN_PERCENT_LIMIT, CADENCE_TAIL_MICROSECOND_LIMIT, CadencePool, CadenceVerdict,
    cadence_verdict, median_confidence_bounds, percentile,
};

use super::*;
use crate::{
    dispatch::Outcome,
    outbox,
    pane_observations::label_write_timeout_message,
    ports::testing::{EmptyPortScan, FakeSupervisor, FixedClock},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_pane_state::RecordedRow;
use soyeht_ptyd::wire::{Output, Response};
use std::{collections::VecDeque, fs, time::Instant};
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

/// Pools one window's steady samples and returns the median verdict so far.
fn pool_cadence_window(
    pool: &mut CadencePool,
    window: usize,
    off_micros: &[u128],
    on_micros: &[u128],
) -> CadenceVerdict {
    let steady_off = &off_micros[CADENCE_WARMUP_PAIRS..];
    let steady_on = &on_micros[CADENCE_WARMUP_PAIRS..];
    pool.add_window(steady_off, steady_on);
    let ratios = pool.paired_ratios_per_mille();
    let verdict = cadence_verdict(&ratios, window == CADENCE_MAX_ATTEMPTS);
    eprintln!(
        "capture cadence pooled attempt={window} pairs={} median_ratio_per_mille={} confidence_bounds={:?} verdict={verdict:?} tail_windows={} tail_violations={}",
        ratios.len(),
        percentile(&ratios, 1, 2),
        median_confidence_bounds(&ratios),
        pool.tail_windows,
        pool.tail_violations
    );
    verdict
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
fn terminal_observation_and_hook_label_share_one_attributable_stream() {
    let directory = std::env::temp_dir().join(format!("sn-terminal-label-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    soyeht_platform::fs::set_private_dir(&directory).unwrap();
    let harness = HarnessIdentity::known("test.terminal", None).unwrap();
    let build = EngineBuild {
        package_version: "0.1.0".into(),
        executable_sha256: "a".repeat(64),
    };
    let capture = TerminalCapture::open(
        &directory,
        "pane-1",
        harness.clone(),
        7,
        4,
        40,
        build.clone(),
    )
    .unwrap();
    capture
        .record(10, Arc::new(b"working\r\n".to_vec()))
        .unwrap();
    capture
        .record_label(
            11,
            PaneState::Working,
            StateSource::Hook,
            GroundTruthProvenance::session_hook(7, "b".repeat(64)),
        )
        .unwrap();
    drop(capture);

    let rows: Vec<_> = SegmentStore::open(
        &directory,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness.clone(),
        build,
    )
    .unwrap()
    .replay()
    .unwrap()
    .into_iter()
    .flat_map(|segment| segment.rows)
    .collect();
    assert!(matches!(
        &rows[0],
        RecordedRow::Observation(frame)
            if frame.pane_id == "pane-1"
                && frame.pane_kind == ObservationPaneKind::Terminal
                && frame.harness == harness
                && frame.session_generation == 7
    ));
    assert!(matches!(
        &rows[1],
        RecordedRow::Label(label)
            if label.pane_id == "pane-1"
                && label.pane_kind == ObservationPaneKind::Terminal
                && label.harness == harness
                && label.provenance.session_generation == 7
                && label.provenance.credential_sha256.as_deref() == Some(&"b".repeat(64))
    ));
    fs::remove_dir_all(directory).unwrap();
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
    crate::test_paths::remove_after(core, directory);
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
    soyeht_platform::fs::set_private_dir(&exports).unwrap();
    let export = exports.join("export-preserved.jsonl");
    fs::write(&export, b"raw terminal export\n").unwrap();
    soyeht_platform::fs::set_private_file(&export).unwrap();

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
    crate::test_paths::remove_after(core, directory);
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
    soyeht_platform::fs::set_private_dir(&directory).unwrap();
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
        1,
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
                    format!("compile unit {run:02}-{line:04} ................................\r\n")
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
    crate::test_panes::replay_rows(
        &directory.join("on/experiments/pane-state"),
        "cadence-pane",
        build,
    )
}

fn cadence_core(root: &Path, chunks: &[Vec<u8>], capture: bool) -> Core {
    fs::create_dir(root).unwrap();
    let mut core = Core::open(root).unwrap();
    // The capture below needs the build, which no test waits for under the lock.
    core.engine_build.wait().unwrap();
    // This benchmark measures poll_output and the capture handoff. Keep
    // unrelated local-server discovery deterministic across retries.
    core.clock = Arc::new(FixedClock(0));
    core.web = crate::web::Service::new(Box::new(EmptyPortScan));
    let pane_id = crate::test_panes::insert_terminal_pane(&mut core, root, "cadence", capture);
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
    core.link_supervisor(Box::new(FakeSupervisor {
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

#[test]
#[ignore = "measured separately by scripts/check_all.sh without competing tests"]
fn frame_dense_worker_headroom_reports_queue_pressure_and_time_coverage() {
    let directory = std::env::temp_dir().join(format!("sn-headroom-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    soyeht_platform::fs::set_private_dir(&directory).unwrap();
    let build = EngineBuild {
        package_version: "0.1.0".into(),
        executable_sha256: "b".repeat(64),
    };
    let capture = TerminalCapture::open(
        &directory,
        "frame-dense-pane",
        HarnessIdentity::Unknown,
        1,
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

/// Times one measurement window of interleaved capture-off and capture-on
/// polls and returns both arms' samples. The recording's correctness
/// (lossless observations, full time coverage, every steady byte) is
/// asserted for every window, whatever its timing turns out to be.
fn run_cadence_attempt(
    directory: &Path,
    chunks: &[Vec<u8>],
    attempt: usize,
) -> (Vec<u128>, Vec<u128>) {
    fs::create_dir(directory).unwrap();
    let load_average_before = soyeht_platform::testing::load_average();
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
    log_cadence_attempt(
        attempt,
        load_average_before,
        soyeht_platform::testing::load_average(),
        &off_micros,
        &on_micros,
    );
    let build = on.engine_build.ready().unwrap().unwrap();
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
    (off_micros, on_micros)
}

/// Capture-on must keep the median paired poll cadence within 5 percent of
/// capture-off and the p95 within 5 ms. Windows of ten paired samples are
/// pooled until the median's confidence interval clears the limit on one
/// side and at least three windows have judged the p95 rule, so host load
/// costs extra windows instead of a wrong verdict. The recording itself is
/// checked for completeness in every window.
#[test]
#[ignore = "measured separately by scripts/check_all.sh without competing tests"]
fn capture_cadence_measurement_uses_ten_paired_noisy_samples() {
    let directory = std::env::temp_dir().join(format!("sn-cadence-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    let chunks = noisy_chunks();
    let mut pool = CadencePool::default();
    let mut verdict = CadenceVerdict::Undecided;
    let mut attempt = 0;
    while (verdict == CadenceVerdict::Undecided || !pool.tail_resolved())
        && attempt < CADENCE_MAX_ATTEMPTS
    {
        attempt += 1;
        let (off_micros, on_micros) = run_cadence_attempt(
            &directory.join(format!("attempt-{attempt}")),
            &chunks,
            attempt,
        );
        verdict = pool_cadence_window(&mut pool, attempt, &off_micros, &on_micros);
    }
    fs::remove_dir_all(directory).unwrap();
    assert_ne!(
        verdict,
        CadenceVerdict::Implausible,
        "capture-on measured faster than capture-off beyond the noise; the measurement is contaminated"
    );
    assert_eq!(
        verdict,
        CadenceVerdict::WithinLimit,
        "capture-on median paired cadence exceeds capture-off by more than {CADENCE_MEDIAN_PERCENT_LIMIT} percent"
    );
    assert!(
        pool.tail_within_limit(),
        "capture-on p95 exceeded capture-off p95 by more than max({CADENCE_TAIL_MICROSECOND_LIMIT} us, host noise floor) in {} of {} windows",
        pool.tail_violations,
        pool.tail_windows
    );
    eprintln!(
        "capture cadence accepted attempt={attempt} pairs={} steady_off_median_us={} steady_on_median_us={} tail_windows={} tail_violations={}",
        pool.off.len(),
        percentile(&pool.off, 1, 2),
        percentile(&pool.on, 1, 2),
        pool.tail_windows,
        pool.tail_violations
    );
}
