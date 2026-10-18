use super::*;
use soyeht_pane_state::{
    MAX_SCREEN_LINE_BYTES, MAX_SCREEN_LINES, ObservationFrame, ObservationReason,
};
use soyeht_platform::fs::symlink;
use soyeht_protocol::{PaneState, StateSource};
use std::sync::mpsc;
use uuid::Uuid;

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("sn-observations-{}", Uuid::new_v4()));
        create_private_dir(&directory).unwrap();
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
        pane_kind: ObservationPaneKind::Terminal,
        harness: harness(),
        session_generation: 1,
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
        pane_kind: ObservationPaneKind::Terminal,
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
    let store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    let recorder = ObservationRecorder::open(store).unwrap();
    for sequence in 0..8 {
        assert_eq!(
            recorder.try_record(frame(sequence)).unwrap(),
            QueueOutcome::Queued
        );
    }
    recorder.finish().unwrap();
    let replayed = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
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
    let store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    let (recorder, writer_gate) = ObservationRecorder::open_paused(store, 2).unwrap();
    assert_eq!(recorder.try_record(frame(0)).unwrap(), QueueOutcome::Queued);
    assert_eq!(recorder.try_record(frame(1)).unwrap(), QueueOutcome::Queued);
    assert_eq!(
        recorder.try_record(frame(2)).unwrap(),
        QueueOutcome::GapExtended
    );
    writer_gate.wait();
    recorder.finish().unwrap();

    let replayed = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
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
    let store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    let (recorder, writer_gate) = ObservationRecorder::open_paused(store, 2).unwrap();
    recorder.try_record(frame(0)).unwrap();
    recorder.try_record(frame(1)).unwrap();
    assert_eq!(
        recorder.try_record(frame(2)).unwrap(),
        QueueOutcome::GapExtended
    );
    let label = GroundTruthLabel {
        pane_id: "pane-1".into(),
        pane_kind: ObservationPaneKind::Terminal,
        harness: harness(),
        timestamp_ms: 1_800_000_000_003,
        sequence: 3,
        state: PaneState::WaitingForYou,
        source: StateSource::Hook,
        provenance: soyeht_pane_state::GroundTruthProvenance::session_hook(1, "a".repeat(64)),
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

    let rows = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
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
    let store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    let (recorder, writer_gate) = ObservationRecorder::open_paused(store, 1).unwrap();
    recorder.try_record(frame(0)).unwrap();
    let label = GroundTruthLabel {
        pane_id: "pane-1".into(),
        pane_kind: ObservationPaneKind::Terminal,
        harness: harness(),
        timestamp_ms: 1_800_000_000_001,
        sequence: 1,
        state: PaneState::Working,
        source: StateSource::Protocol,
        provenance: soyeht_pane_state::GroundTruthProvenance::protocol(1),
    };

    let error = recorder
        .record_label_with_timeout(label, Duration::from_millis(10))
        .unwrap_err();
    let failure = label_write_timeout_message();
    assert!(error.to_string().contains(&failure));
    assert_eq!(
        recorder.failure().unwrap().as_deref(),
        Some(failure.as_str())
    );
    assert!(recorder.try_record(frame(2)).is_err());

    writer_gate.wait();
    recorder.finish().unwrap();
}

#[test]
fn rotation_replays_every_whole_row_and_repeats_exact_provenance() {
    let fixture = Fixture::new();
    let mut store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
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
    let mut first = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    first.append(observation(0, false)).unwrap();
    drop(first);
    let mut second = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('b'),
    )
    .unwrap();
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
fn incompatible_segment_is_skipped_locally_and_a_new_segment_does_not_collide() {
    let fixture = Fixture::new();
    ensure_private_directory(&fixture.root).unwrap();
    let pane_directory = fixture.root.join(pane_directory_name("pane-1"));
    ensure_private_directory(&pane_directory).unwrap();
    create_segment_file(
        &pane_directory,
        0,
        b"{\"kind\":\"metadata\",\"schemaVersion\":5}\n",
    )
    .unwrap();

    let mut store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    assert_eq!(store.next_sequence().unwrap(), 0);
    store.append(observation(0, false)).unwrap();
    drop(store);

    let replay = replay_pane(&fixture.root, "pane-1").unwrap();
    assert_eq!(
        replay.skipped_segments,
        vec![SkippedSegment {
            pane_id: "pane-1".into(),
            schema_version: 5,
        }]
    );
    assert_eq!(replay.segments.len(), 1);
    assert_eq!(replay.segments[0].metadata.segment_sequence, 1);
    assert_eq!(replay.segments[0].rows, vec![observation(0, false)]);
}

#[test]
fn clean_close_and_abrupt_end_remain_distinguishable_after_reopen() {
    let fixture = Fixture::new();
    let store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    let recorder = ObservationRecorder::open(store).unwrap();
    recorder.try_record(frame(0)).unwrap();
    recorder.finish().unwrap();
    let clean = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap()
    .replay()
    .unwrap();
    assert_eq!(clean.len(), 1);
    assert_eq!(clean[0].clean_close.as_ref().unwrap().last_sequence, 0);

    let mut abrupt = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    abrupt.append(observation(1, false)).unwrap();
    drop(abrupt);
    let replayed = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
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
    let mut store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
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
    let mut store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
    store.append(observation(0, false)).unwrap();
    let pane_directory = fixture.root.join(pane_directory_name("pane-1"));
    assert!(dir_is_owner_only(&fixture.root).unwrap());
    let segment = validated_segment_files(&pane_directory).unwrap().remove(0);
    assert!(file_is_owner_only(&File::open(&segment).unwrap()).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&fixture.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&segment).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
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
    let mut store = SegmentStore::open(
        &fixture.root,
        "pane-1",
        ObservationPaneKind::Terminal,
        harness(),
        engine('a'),
    )
    .unwrap();
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
    assert!(
        SegmentStore::open(
            &fixture.root,
            "pane-1",
            ObservationPaneKind::Terminal,
            harness(),
            engine('a')
        )
        .is_err()
    );

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
