#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_pane_state::{
    Confidence, Detection, Detector, GroundTruthLabel, GroundTruthProvenance, HarnessIdentity,
    MIN_GROUND_TRUTH_TRANSITIONS, MIN_RATE_DENOMINATOR, ObservationFrame, ObservationGap,
    ObservationPaneKind, ObservationReason, RecordedRow, evaluate, evaluate_with_omitted_panes,
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
    label_from(sequence, timestamp_ms, state, StateSource::Protocol)
}

fn label_from(
    sequence: u64,
    timestamp_ms: i64,
    state: PaneState,
    source: StateSource,
) -> RecordedRow {
    RecordedRow::Label(GroundTruthLabel {
        pane_id: "pane-1".into(),
        pane_kind: soyeht_pane_state::ObservationPaneKind::Terminal,
        harness: harness(),
        timestamp_ms,
        sequence,
        state,
        source: source.clone(),
        provenance: if source == StateSource::Hook {
            GroundTruthProvenance::session_hook(1, "a".repeat(64))
        } else {
            GroundTruthProvenance::protocol(1)
        },
    })
}

fn scoring_fixture(source: StateSource) -> Vec<RecordedRow> {
    let mut rows = Vec::new();
    let mut sequence = 0;
    let mut timestamp_ms = 0;
    rows.push(label_from(
        sequence,
        timestamp_ms,
        PaneState::Idle,
        source.clone(),
    ));
    sequence += 1;
    timestamp_ms += 10;
    rows.push(observation(sequence, timestamp_ms, "idle"));
    sequence += 1;
    timestamp_ms += 10;
    for _ in 0..MIN_RATE_DENOMINATOR {
        for (state, screen) in [
            (PaneState::Working, "working"),
            (PaneState::WaitingForYou, "waiting"),
            (PaneState::Idle, "idle"),
        ] {
            rows.push(label_from(sequence, timestamp_ms, state, source.clone()));
            sequence += 1;
            timestamp_ms += 10;
            rows.push(observation(sequence, timestamp_ms, screen));
            sequence += 1;
            timestamp_ms += 10;
        }
    }
    rows
}

fn state_metrics(
    metrics: &soyeht_pane_state::HarnessMetrics,
    wanted: PaneState,
) -> &soyeht_pane_state::StateMetrics {
    metrics
        .states
        .iter()
        .find(|state| state.state == wanted)
        .unwrap()
}

fn observation(sequence: u64, timestamp_ms: i64, screen: &str) -> RecordedRow {
    RecordedRow::Observation(ObservationFrame {
        pane_id: "pane-1".into(),
        pane_kind: soyeht_pane_state::ObservationPaneKind::Terminal,
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
        pane_kind: soyeht_pane_state::ObservationPaneKind::Terminal,
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
    let rows = scoring_fixture(StateSource::Protocol);
    let oracle = evaluate(&ScreenOracle, &rows).unwrap();
    let harness = &oracle.harnesses[0];
    assert_eq!(harness.ground_truth_source, Some(StateSource::Protocol));
    assert_eq!(harness.observations, 61);
    assert_eq!(harness.labels, 61);
    assert_eq!(harness.label_transitions, 60);
    assert_eq!(harness.labeled_observations, 61);
    assert!(harness.scores_available);
    assert_eq!(harness.unknown_rate.value, Some(0.0));
    assert_eq!(harness.time_coverage.as_ref().unwrap().value, Some(1.0));
    assert_eq!(harness.unclassified_gap_ms, 0);
    for state in &harness.states {
        assert_eq!(state.truth_transitions, MIN_GROUND_TRUTH_TRANSITIONS);
        assert_eq!(state.precision.value, Some(1.0));
        assert_eq!(state.recall.value, Some(1.0));
        assert_eq!(state.latency.median_ms, Some(10));
    }
    let idle = harness
        .states
        .iter()
        .find(|state| state.state == PaneState::Idle)
        .unwrap();
    assert_eq!(idle.time_coverage.as_ref().unwrap().value, Some(1.0));

    let floor = evaluate(&AlwaysUnknown, &rows).unwrap();
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
fn pane_populations_stay_separate_and_unlabeled_groups_report_no_scores() {
    let mut rows = scoring_fixture(StateSource::Protocol);
    let mut agent = match observation(0, 5, "working") {
        RecordedRow::Observation(frame) => frame,
        _ => unreachable!(),
    };
    agent.pane_id = "agent-pane".into();
    agent.pane_kind = ObservationPaneKind::Agent;
    rows.push(RecordedRow::Observation(agent));

    let report = evaluate(&ScreenOracle, &rows).unwrap();
    assert_eq!(report.harnesses.len(), 2);
    let agent = report
        .harnesses
        .iter()
        .find(|metrics| metrics.pane_kind == ObservationPaneKind::Agent)
        .unwrap();
    let terminal = report
        .harnesses
        .iter()
        .find(|metrics| metrics.pane_kind == ObservationPaneKind::Terminal)
        .unwrap();
    assert_eq!(agent.harness, terminal.harness);
    assert_eq!(agent.ground_truth_source, None);
    assert_eq!(agent.observations, 1);
    assert_eq!(agent.labels, 0);
    assert_eq!(agent.label_transitions, 0);
    assert_eq!(agent.labeled_observations, 0);
    assert!(!agent.scores_available);
    assert!(agent.states.is_empty());
    assert_eq!(terminal.observations, 61);
    assert_eq!(terminal.ground_truth_source, Some(StateSource::Protocol));
    assert_eq!(terminal.labeled_observations, 61);
    assert!(terminal.scores_available);
}

#[test]
fn omitted_sources_disable_only_elapsed_coverage_and_mark_the_partial_group() {
    let mut rows = scoring_fixture(StateSource::Protocol);
    let mut complete = scoring_fixture(StateSource::Protocol);
    let complete_harness =
        HarnessIdentity::known("registry.complete", Some("1.0.0".into())).unwrap();
    for row in &mut complete {
        match row {
            RecordedRow::Observation(frame) => {
                frame.pane_id = "pane-complete".into();
                frame.harness = complete_harness.clone();
            }
            RecordedRow::Label(label) => {
                label.pane_id = "pane-complete".into();
                label.harness = complete_harness.clone();
            }
            RecordedRow::Gap(gap) => {
                gap.pane_id = "pane-complete".into();
                gap.harness = complete_harness.clone();
            }
        }
    }
    rows.extend(complete);

    let report = evaluate_with_omitted_panes(&ScreenOracle, &rows, &["pane-1".into()]).unwrap();
    let partial = report
        .harnesses
        .iter()
        .find(|metrics| metrics.harness == harness())
        .unwrap();
    assert!(partial.scores_available);
    assert_eq!(partial.panes_with_omitted_sources, 1);
    assert_eq!(partial.time_coverage, None);
    assert!(
        partial
            .states
            .iter()
            .all(|state| state.time_coverage.is_none())
    );
    assert!(
        partial
            .states
            .iter()
            .all(|state| state.precision.value == Some(1.0))
    );
    assert!(
        partial
            .states
            .iter()
            .all(|state| state.recall.value == Some(1.0))
    );

    let complete = report
        .harnesses
        .iter()
        .find(|metrics| metrics.harness == complete_harness)
        .unwrap();
    assert_eq!(complete.panes_with_omitted_sources, 0);
    assert_eq!(complete.time_coverage.as_ref().unwrap().value, Some(1.0));
}

#[test]
fn protocol_and_hook_truth_for_one_harness_never_share_a_rate() {
    let mut rows = Vec::new();
    let mut sequence = 0;
    for row in scoring_fixture(StateSource::Protocol) {
        match row {
            RecordedRow::Label(mut label) => {
                label.sequence = sequence;
                sequence += 1;
                let mut hook = label.clone();
                hook.sequence = sequence;
                hook.source = StateSource::Hook;
                hook.provenance = GroundTruthProvenance::session_hook(1, "a".repeat(64));
                hook.state = match hook.state {
                    PaneState::Working => PaneState::Idle,
                    PaneState::Idle => PaneState::Working,
                    state => state,
                };
                sequence += 1;
                rows.push(RecordedRow::Label(label));
                rows.push(RecordedRow::Label(hook));
            }
            RecordedRow::Observation(mut frame) => {
                frame.sequence = sequence;
                sequence += 1;
                rows.push(RecordedRow::Observation(frame));
            }
            RecordedRow::Gap(_) => unreachable!(),
        }
    }

    let report = evaluate(&ScreenOracle, &rows).unwrap();
    assert_eq!(report.harnesses.len(), 2);
    let protocol = report
        .harnesses
        .iter()
        .find(|metrics| metrics.ground_truth_source == Some(StateSource::Protocol))
        .unwrap();
    let hook = report
        .harnesses
        .iter()
        .find(|metrics| metrics.ground_truth_source == Some(StateSource::Hook))
        .unwrap();
    assert_eq!(
        state_metrics(protocol, PaneState::Working).recall.value,
        Some(1.0)
    );
    assert_eq!(
        state_metrics(hook, PaneState::Working).recall.value,
        Some(0.0)
    );
    assert_eq!(
        state_metrics(protocol, PaneState::Idle).recall.value,
        Some(1.0)
    );
    assert_eq!(state_metrics(hook, PaneState::Idle).recall.value, Some(0.0));
}

#[test]
fn repeated_labels_and_frames_cannot_replace_twenty_ground_truth_transitions() {
    let mut rows = Vec::new();
    for index in 0..=MIN_GROUND_TRUTH_TRANSITIONS {
        let sequence = index * 2;
        let timestamp = sequence as i64 * 10;
        rows.push(label(sequence, timestamp, PaneState::Working));
        rows.push(observation(sequence + 1, timestamp + 10, "working"));
    }
    let metrics = &evaluate(&ScreenOracle, &rows).unwrap().harnesses[0];
    assert_eq!(metrics.labels, MIN_GROUND_TRUTH_TRANSITIONS + 1);
    assert_eq!(metrics.label_transitions, 0);
    assert_eq!(
        metrics.states[0].recall.denominator,
        MIN_GROUND_TRUTH_TRANSITIONS + 1
    );
    assert_eq!(metrics.states[0].recall.value, None);
    assert_eq!(metrics.states[0].duration.measured, 0);
    assert_eq!(metrics.states[0].duration.unavailable, 1);
    assert!(!metrics.scores_available);
}

#[test]
fn twenty_transitions_without_one_publishable_rate_keep_scores_unavailable() {
    let mut rows = Vec::new();
    for index in 0..=MIN_GROUND_TRUTH_TRANSITIONS {
        let (state, screen) = match index % 3 {
            0 => (PaneState::Working, "working"),
            1 => (PaneState::WaitingForYou, "waiting"),
            _ => (PaneState::Idle, "idle"),
        };
        let sequence = index * 2;
        let timestamp = sequence as i64 * 10;
        rows.push(label(sequence, timestamp, state));
        rows.push(observation(sequence + 1, timestamp + 10, screen));
    }
    let metrics = &evaluate(&ScreenOracle, &rows).unwrap().harnesses[0];
    assert_eq!(metrics.label_transitions, MIN_GROUND_TRUTH_TRANSITIONS);
    assert!(
        metrics
            .states
            .iter()
            .all(|state| { state.precision.value.is_none() && state.recall.value.is_none() })
    );
    assert!(!metrics.scores_available);
}

#[test]
fn a_ready_group_still_suppresses_each_under_sampled_state_rate() {
    let mut rows = scoring_fixture(StateSource::Protocol);
    for row in &mut rows {
        if let RecordedRow::Observation(frame) = row
            && frame.lines[0] == "waiting"
            && frame.sequence > 5
        {
            frame.lines[0] = "working".into();
        }
    }
    let metrics = &evaluate(&ScreenOracle, &rows).unwrap().harnesses[0];
    assert!(metrics.label_transitions >= MIN_GROUND_TRUTH_TRANSITIONS);
    assert!(metrics.scores_available);
    let waiting = metrics
        .states
        .iter()
        .find(|state| state.state == PaneState::WaitingForYou)
        .unwrap();
    assert_eq!(waiting.precision.denominator, 1);
    assert_eq!(waiting.precision.value, None);
    assert_eq!(waiting.recall.denominator, MIN_RATE_DENOMINATOR);
    assert_eq!(waiting.recall.value, Some(0.05));
}

#[test]
fn fast_real_transitions_publish_a_flapping_distribution_without_inventing_a_cutoff() {
    let metrics = &evaluate(&ScreenOracle, &scoring_fixture(StateSource::Protocol))
        .unwrap()
        .harnesses[0];
    assert!(metrics.scores_available);
    for state in &metrics.states {
        assert_eq!(state.duration.measured, MIN_GROUND_TRUTH_TRANSITIONS);
        assert_eq!(state.duration.minimum_ms, Some(20));
        assert_eq!(state.duration.median_ms, Some(20));
        assert_eq!(
            state.duration.entries_under_one_second,
            MIN_GROUND_TRUTH_TRANSITIONS
        );
    }
    let idle = state_metrics(metrics, PaneState::Idle);
    assert_eq!(idle.duration.unavailable, 1);
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
