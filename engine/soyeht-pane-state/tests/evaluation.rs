#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_pane_state::{
    Confidence, Detection, Detector, DetectorRegistry, EngineBuild, EvaluationDataset,
    EvaluationOmittedSource, EvaluationSource, GroundTruthLabel, GroundTruthProvenance,
    HarnessIdentity, ObservationFrame, ObservationPaneKind, ObservationReason, RecordedRow,
};
use soyeht_protocol::{PaneState, StateSource};

struct ScreenDetector;

impl Detector for ScreenDetector {
    fn state(&self, frame: &ObservationFrame) -> Detection {
        let state = match frame.lines.first().map(String::as_str) {
            Some("working") => PaneState::Working,
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
                rule: Some("screen_fixture"),
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

fn build(digest: char) -> EngineBuild {
    EngineBuild {
        package_version: "0.1.0".into(),
        executable_sha256: digest.to_string().repeat(64),
    }
}

fn label(sequence: u64, state: PaneState) -> RecordedRow {
    RecordedRow::Label(GroundTruthLabel {
        pane_id: "pane-1".into(),
        pane_kind: ObservationPaneKind::Terminal,
        harness: harness(),
        timestamp_ms: i64::try_from(sequence * 10).unwrap(),
        sequence,
        state,
        source: StateSource::Protocol,
        provenance: GroundTruthProvenance::protocol(1),
    })
}

fn observation(sequence: u64, screen: &str) -> RecordedRow {
    RecordedRow::Observation(ObservationFrame {
        pane_id: "pane-1".into(),
        pane_kind: ObservationPaneKind::Terminal,
        harness: harness(),
        session_generation: 1,
        timestamp_ms: i64::try_from(sequence * 10).unwrap(),
        sequence,
        reason: ObservationReason::VisualChange,
        osc_title: None,
        lines: vec![screen.into()],
        cursor: None,
        foreground_process: None,
        bytes_per_second: 1,
    })
}

fn rows() -> Vec<RecordedRow> {
    vec![
        label(0, PaneState::Idle),
        observation(1, "idle"),
        label(2, PaneState::Working),
        observation(3, "working"),
    ]
}

fn source(digest: char) -> EvaluationSource {
    source_range(digest, 0, 0, 3)
}

fn source_range(
    digest: char,
    segment_sequence: u64,
    first_sequence: u64,
    last_sequence: u64,
) -> EvaluationSource {
    EvaluationSource {
        pane_id: "pane-1".into(),
        pane_kind: ObservationPaneKind::Terminal,
        harness: harness(),
        engine: build(digest),
        segment_sequence,
        first_sequence,
        last_sequence,
        clean_close: true,
    }
}

fn omitted(pane_id: &str) -> EvaluationOmittedSource {
    EvaluationOmittedSource {
        pane_id: pane_id.into(),
        schema_version: 7,
        segments: 1,
    }
}

#[test]
fn one_run_scores_every_registered_detector_against_one_dataset_identity() {
    let dataset = EvaluationDataset::new(vec![source('a')], rows(), vec![]).unwrap();
    let report = DetectorRegistry::new()
        .with("screen_fixture", ScreenDetector)
        .unwrap()
        .with("always_unknown", AlwaysUnknown)
        .unwrap()
        .evaluate(&dataset)
        .unwrap();

    assert_eq!(
        report
            .detectors
            .iter()
            .map(|detector| detector.name.as_str())
            .collect::<Vec<_>>(),
        ["screen_fixture", "always_unknown"]
    );
    assert_eq!(
        report.dataset_fingerprint_sha256,
        dataset.fingerprint_sha256()
    );
    assert_eq!(report.engine_builds, [build('a')]);
    assert_eq!(report.counts.rows, 4);
    assert_eq!(report.counts.observations, 2);
    assert_eq!(report.counts.labels, 2);
    assert_eq!(report.counts.gaps, 0);
    assert_eq!(report.counts.omitted_panes, 0);
    assert_eq!(
        report.detectors[0].metrics.harnesses[0].unknown_rate.value,
        Some(0.0)
    );
    assert_eq!(
        report.detectors[1].metrics.harnesses[0].unknown_rate.value,
        Some(1.0)
    );
}

#[test]
fn fingerprint_is_stable_for_set_order_and_changes_with_measured_data() {
    let early = source_range('a', 0, 0, 1);
    let late = source_range('b', 1, 2, 3);
    let first = EvaluationDataset::new(
        vec![late.clone(), early.clone()],
        rows(),
        vec![omitted("pane-old"), omitted("pane-older")],
    )
    .unwrap();
    let reordered = EvaluationDataset::new(
        vec![early.clone(), late.clone()],
        rows(),
        vec![omitted("pane-older"), omitted("pane-old")],
    )
    .unwrap();
    assert_eq!(first.fingerprint_sha256(), reordered.fingerprint_sha256());

    let mut changed_rows = rows();
    let RecordedRow::Observation(frame) = &mut changed_rows[1] else {
        panic!("fixture row changed kind")
    };
    frame.lines = vec!["different".into()];
    let changed = EvaluationDataset::new(
        vec![early, late],
        changed_rows,
        vec![omitted("pane-old"), omitted("pane-older")],
    )
    .unwrap();
    assert_ne!(first.fingerprint_sha256(), changed.fingerprint_sha256());
}

#[test]
fn every_row_is_bound_to_exactly_one_source_build() {
    let mut wrong_build_source = source('a');
    wrong_build_source.last_sequence = 1;
    assert_eq!(
        EvaluationDataset::new(vec![wrong_build_source], rows(), vec![]).unwrap_err(),
        soyeht_pane_state::Error::MismatchedProvenance
    );

    let overlapping = source('b');
    assert_eq!(
        EvaluationDataset::new(vec![source('a'), overlapping], rows(), vec![]).unwrap_err(),
        soyeht_pane_state::Error::MismatchedProvenance
    );

    let first = EvaluationDataset::new(vec![source('a')], rows(), vec![]).unwrap();
    let second = EvaluationDataset::new(vec![source('b')], rows(), vec![]).unwrap();
    assert_ne!(first.fingerprint_sha256(), second.fingerprint_sha256());
}

#[test]
fn report_comparability_requires_the_same_engine_builds() {
    let dataset = EvaluationDataset::new(vec![source('a')], rows(), vec![]).unwrap();
    let report = DetectorRegistry::shipped().unwrap().evaluate(&dataset).unwrap();
    let mut different_build = report.clone();
    different_build.engine_builds = vec![build('b')];

    assert!(report.same_dataset_as(&report));
    assert!(!report.same_dataset_as(&different_build));
}

#[test]
fn report_comparability_requires_the_same_counts() {
    let dataset = EvaluationDataset::new(vec![source('a')], rows(), vec![]).unwrap();
    let report = DetectorRegistry::shipped().unwrap().evaluate(&dataset).unwrap();
    let mut different_counts = report.clone();
    different_counts.counts.rows += 1;

    assert!(report.same_dataset_as(&report));
    assert!(!report.same_dataset_as(&different_counts));
}

#[test]
fn shipped_registry_names_the_authored_baseline() {
    assert_eq!(
        DetectorRegistry::shipped()
            .unwrap()
            .names()
            .collect::<Vec<_>>(),
        ["regex_baseline"]
    );
}
