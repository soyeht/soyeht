#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_pane_state::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, Error, GroundTruthLabel, HarnessIdentity,
    MAX_SEGMENT_BYTES, ObservationFrame, ObservationGap, RecordedRow, SegmentMetadata,
    decode_segment, encode_row,
};
use soyeht_protocol::{PaneState, StateSource};

fn harness() -> HarnessIdentity {
    HarnessIdentity::known("registry.test", Some("1.2.3".into())).unwrap()
}

fn metadata() -> SegmentMetadata {
    SegmentMetadata {
        schema_version: DATASET_SCHEMA_VERSION,
        pane_id: "pane-1".into(),
        harness: harness(),
        engine: EngineBuild {
            package_version: "0.1.0".into(),
            executable_sha256: "a".repeat(64),
        },
        segment_sequence: 7,
        started_at_ms: 1_800_000_000_000,
    }
}

fn observation(sequence: u64) -> ObservationFrame {
    ObservationFrame {
        pane_id: "pane-1".into(),
        harness: harness(),
        timestamp_ms: 1_800_000_000_000 + sequence as i64,
        sequence,
        osc_title: Some("working".into()),
        lines: vec!["line one".into(), "line two".into()],
        cursor: None,
        foreground_process: Some("test".into()),
        bytes_per_second: 42,
    }
}

fn label(sequence: u64) -> GroundTruthLabel {
    GroundTruthLabel {
        pane_id: "pane-1".into(),
        harness: harness(),
        timestamp_ms: 1_800_000_000_000 + sequence as i64,
        sequence,
        state: PaneState::Working,
        source: StateSource::Protocol,
    }
}

fn gap(first_sequence: u64, last_sequence: u64, dropped_observations: u64) -> ObservationGap {
    ObservationGap {
        pane_id: "pane-1".into(),
        harness: harness(),
        first_timestamp_ms: 1_800_000_000_100,
        last_timestamp_ms: 1_800_000_000_050,
        first_sequence,
        last_sequence,
        dropped_observations,
    }
}

fn segment(rows: &[DatasetRow]) -> Vec<u8> {
    rows.iter()
        .flat_map(|row| encode_row(row).unwrap())
        .collect()
}

#[test]
fn segment_round_trip_keeps_header_observation_label_and_gap_distinct() {
    let bytes = segment(&[
        DatasetRow::Metadata(metadata()),
        DatasetRow::Observation(observation(8)),
        DatasetRow::Label(label(9)),
        DatasetRow::Gap(gap(10, 12, 2)),
    ]);
    let replayed = decode_segment(&bytes).unwrap();
    assert_eq!(replayed.metadata, metadata());
    assert_eq!(
        replayed.rows,
        vec![
            RecordedRow::Observation(observation(8)),
            RecordedRow::Label(label(9)),
            RecordedRow::Gap(gap(10, 12, 2)),
        ]
    );
    assert!(bytes.starts_with(b"{\"kind\":\"metadata\""));
}

#[test]
fn gap_requires_a_nonempty_count_inside_its_sequence_interval() {
    assert_eq!(gap(10, 12, 2).validate(), Ok(()));
    assert_eq!(gap(10, 12, 0).validate(), Err(Error::InvalidGap));
    assert_eq!(gap(10, 12, 4).validate(), Err(Error::InvalidGap));
    assert_eq!(gap(12, 10, 1).validate(), Err(Error::InvalidGap));
}

#[test]
fn replay_uses_sequence_when_the_wall_clock_moves_backwards() {
    let mut rolled_back = observation(8);
    rolled_back.timestamp_ms = metadata().started_at_ms - 1_000;
    let replayed = decode_segment(&segment(&[
        DatasetRow::Metadata(metadata()),
        DatasetRow::Observation(rolled_back.clone()),
    ]))
    .unwrap();
    assert_eq!(replayed.rows, vec![RecordedRow::Observation(rolled_back)]);
}

#[test]
fn replay_rejects_missing_or_repeated_headers_and_future_kinds() {
    assert_eq!(
        decode_segment(&segment(&[DatasetRow::Observation(observation(1))])),
        Err(Error::InvalidHeader)
    );
    assert_eq!(
        decode_segment(&segment(&[
            DatasetRow::Metadata(metadata()),
            DatasetRow::Metadata(metadata())
        ])),
        Err(Error::InvalidHeader)
    );
    let future = format!(
        "{}{{\"kind\":\"future\"}}\n",
        String::from_utf8(encode_row(&DatasetRow::Metadata(metadata())).unwrap()).unwrap()
    );
    assert_eq!(decode_segment(future.as_bytes()), Err(Error::Json));
    let unknown_field = String::from_utf8(encode_row(&DatasetRow::Metadata(metadata())).unwrap())
        .unwrap()
        .replacen("}\n", ",\"futureField\":true}\n", 1);
    assert_eq!(decode_segment(unknown_field.as_bytes()), Err(Error::Json));
}

#[test]
fn replay_rejects_mismatched_provenance_nonmonotonic_rows_and_partial_writes() {
    let mut wrong = observation(8);
    wrong.pane_id = "pane-2".into();
    assert_eq!(
        decode_segment(&segment(&[
            DatasetRow::Metadata(metadata()),
            DatasetRow::Observation(wrong)
        ])),
        Err(Error::MismatchedProvenance)
    );
    assert_eq!(
        decode_segment(&segment(&[
            DatasetRow::Metadata(metadata()),
            DatasetRow::Label(label(9)),
            DatasetRow::Observation(observation(8))
        ])),
        Err(Error::InvalidSequence)
    );
    let mut partial = segment(&[DatasetRow::Metadata(metadata())]);
    partial.pop();
    assert_eq!(decode_segment(&partial), Err(Error::IncompleteRecord));
}

#[test]
fn build_digest_and_schema_are_closed() {
    let mut invalid = metadata();
    invalid.engine.executable_sha256 = "A".repeat(64);
    assert_eq!(invalid.validate(), Err(Error::InvalidDigest));
    invalid = metadata();
    invalid.schema_version += 1;
    assert_eq!(invalid.validate(), Err(Error::SchemaVersion));
    assert_eq!(
        decode_segment(&vec![b'x'; MAX_SEGMENT_BYTES + 1]),
        Err(Error::Limit("dataset segment"))
    );
}
