#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_pane_state::{
    Confidence, Detection, Detector, GroundTruthLabel, HarnessIdentity, ObservationFrame,
    ObservationGap, ObservationReason, RecordedRow, evaluate,
};
use soyeht_protocol::{PaneState, StateSource};

struct ScreenOracle;

impl Detector for ScreenOracle {
    fn state(&self, frame: &ObservationFrame) -> Detection {
        let state = match frame.lines.first().map(String::as_str) {
            Some("working") => PaneState::Working,
            Some("waiting") => PaneState::WaitingForYou,
            Some("idle") => PaneState::Idle,
            _ => PaneState::Unknown,
        };
        if state == PaneState::Unknown {
            Detection::unknown()
        } else {
            Detection {
                state,
                source: StateSource::Screen,
                confidence: Confidence::from_basis_points(10_000).unwrap(),
                rule: Some("oracle_fixture"),
            }
        }
    }
}

struct AlwaysUnknown;

impl Detector for AlwaysUnknown {
    fn state(&self, _: &ObservationFrame) -> Detection {
        Detection::unknown()
    }
}

fn harness() -> HarnessIdentity {
    HarnessIdentity::known("registry.test", Some("1.0.0".into())).unwrap()
}

fn label(sequence: u64, timestamp_ms: i64, state: PaneState) -> RecordedRow {
    RecordedRow::Label(GroundTruthLabel {
        pane_id: "pane-1".into(),
        harness: harness(),
        timestamp_ms,
        sequence,
        state,
        source: StateSource::Protocol,
    })
}

fn observation(sequence: u64, timestamp_ms: i64, screen: &str) -> RecordedRow {
    RecordedRow::Observation(ObservationFrame {
        pane_id: "pane-1".into(),
        harness: harness(),
        timestamp_ms,
        sequence,
        reason: ObservationReason::VisualChange,
        osc_title: None,
        lines: vec![screen.into()],
        cursor: None,
        foreground_process: None,
        bytes_per_second: 1,
    })
}

fn gap(first_sequence: u64, last_sequence: u64, start: i64, end: i64) -> RecordedRow {
    RecordedRow::Gap(ObservationGap {
        pane_id: "pane-1".into(),
        harness: harness(),
        first_timestamp_ms: start,
        last_timestamp_ms: end,
        first_sequence,
        last_sequence,
        dropped_observations: Some(last_sequence - first_sequence + 1),
        dropped_bytes: None,
    })
}

fn fixture(gap_start: i64, gap_end: i64) -> Vec<RecordedRow> {
    vec![
        label(0, 0, PaneState::Working),
        observation(1, 10, "working"),
        observation(2, 20, "working"),
        label(3, 30, PaneState::WaitingForYou),
        observation(4, 40, "waiting"),
        label(5, 50, PaneState::Idle),
        observation(6, 60, "idle"),
        gap(7, 8, gap_start, gap_end),
        label(9, 90, PaneState::Idle),
        observation(10, 100, "idle"),
    ]
}

#[test]
fn oracle_reaches_the_ceiling_and_always_unknown_reaches_the_floor() {
    let oracle = evaluate(&ScreenOracle, &fixture(70, 80)).unwrap();
    let harness = &oracle.harnesses[0];
    assert_eq!(harness.observations, 5);
    assert_eq!(harness.labeled_observations, 5);
    assert_eq!(harness.unknown_rate.value, Some(0.0));
    assert_eq!(harness.time_coverage.as_ref().unwrap().value, Some(0.9));
    assert_eq!(harness.unclassified_gap_ms, 0);
    for state in &harness.states {
        assert_eq!(state.precision.value, Some(1.0));
        assert_eq!(state.recall.value, Some(1.0));
        assert_eq!(state.latency.median_ms, Some(10));
    }
    let idle = harness
        .states
        .iter()
        .find(|state| state.state == PaneState::Idle)
        .unwrap();
    assert_eq!(idle.time_coverage.as_ref().unwrap().value, Some(0.8));

    let floor = evaluate(&AlwaysUnknown, &fixture(70, 80)).unwrap();
    assert_eq!(floor.harnesses[0].unknown_rate.value, Some(1.0));
    assert!(
        floor.harnesses[0]
            .states
            .iter()
            .all(|state| state.recall.value == Some(0.0))
    );
    assert!(oracle.can_rank_against(&floor));
}

#[test]
fn candidate_ranking_refuses_materially_different_coverage() {
    let full = evaluate(&ScreenOracle, &fixture(70, 70)).unwrap();
    let ninety_five = evaluate(&ScreenOracle, &fixture(70, 75)).unwrap();
    let just_below_ninety_five = evaluate(&ScreenOracle, &fixture(70, 76)).unwrap();
    let ninety = evaluate(&ScreenOracle, &fixture(70, 80)).unwrap();
    let eighty = evaluate(&ScreenOracle, &fixture(70, 90)).unwrap();
    assert!(full.can_rank_against(&full));
    assert!(full.can_rank_against(&ninety_five));
    assert!(!full.can_rank_against(&just_below_ninety_five));
    assert!(!full.can_rank_against(&ninety));
    assert!(!ninety.can_rank_against(&eighty));
}

#[test]
fn gaps_are_attributed_only_when_labels_on_both_sides_agree() {
    let mut rows = fixture(70, 80);
    let RecordedRow::Label(after) = &mut rows[8] else {
        panic!("fixture label missing");
    };
    after.state = PaneState::Working;
    let report = evaluate(&ScreenOracle, &rows).unwrap();
    let harness = &report.harnesses[0];
    assert_eq!(harness.unclassified_gap_ms, 10);
    let idle = harness
        .states
        .iter()
        .find(|state| state.state == PaneState::Idle)
        .unwrap();
    assert_eq!(idle.time_coverage.as_ref().unwrap().value, Some(1.0));
    assert_eq!(idle.time_coverage.as_ref().unwrap().numerator, 30);
    assert_eq!(idle.time_coverage.as_ref().unwrap().denominator, 30);
}

#[test]
fn a_clock_rollback_makes_elapsed_metrics_unavailable_without_hiding_counts() {
    let mut rows = fixture(70, 80);
    let RecordedRow::Gap(gap) = &mut rows[7] else {
        panic!("fixture gap missing");
    };
    gap.first_timestamp_ms = 80;
    gap.last_timestamp_ms = 70;
    let report = evaluate(&ScreenOracle, &rows).unwrap();
    let harness = &report.harnesses[0];
    assert_eq!(harness.time_coverage, None);
    assert_eq!(harness.clock_discontinuities, 1);
    assert_eq!(harness.observations, 5);
    assert_eq!(harness.unknown_rate.value, Some(0.0));
}
