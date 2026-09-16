#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_pane_state::{
    Confidence, Detection, Detector, Error, GroundTruthLabel, GroundTruthProvenance,
    HarnessIdentity, ObservationFrame, ObservationReason, RecordedRow, RegexBaseline,
};
use soyeht_protocol::{PaneState, StateSource};

fn frame(harness: &str, title: &str, screen: &str) -> ObservationFrame {
    ObservationFrame {
        pane_id: "pane-1".into(),
        pane_kind: soyeht_pane_state::ObservationPaneKind::Terminal,
        harness: HarnessIdentity::known(harness, Some("1".into())).unwrap(),
        timestamp_ms: 1_800_000_000_000,
        sequence: 1,
        reason: ObservationReason::VisualChange,
        osc_title: (!title.is_empty()).then(|| title.into()),
        lines: screen.lines().map(str::to_owned).collect(),
        cursor: None,
        foreground_process: Some(harness.into()),
        bytes_per_second: 42,
    }
}

#[test]
fn baseline_recognises_reference_prompts_without_guessing_at_plain_text() {
    let baseline = RegexBaseline::new().unwrap();
    let fixtures = [
        (
            "registry.claude-acp",
            "",
            "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel · Enter to confirm",
            PaneState::WaitingForYou,
        ),
        (
            "registry.codex-acp",
            "",
            "› fix the tests\n• Working (4s • esc to interrupt)",
            PaneState::Working,
        ),
        (
            "registry.opencode",
            "",
            "△ Permission required\n  bash: remove build output",
            PaneState::WaitingForYou,
        ),
        ("registry.codex-acp", "codex", "• Done\n›", PaneState::Idle),
    ];
    for (harness, title, screen, expected) in fixtures {
        assert_eq!(
            baseline.state(&frame(harness, title, screen)).state,
            expected
        );
    }
    assert_eq!(
        baseline
            .state(&frame(
                "registry.claude-acp",
                "",
                "some scrollback text without a live prompt"
            ))
            .state,
        PaneState::Unknown
    );
}

#[test]
fn waiting_requires_an_actionable_request_and_any_later_signal_clears_it() {
    let baseline = RegexBaseline::new().unwrap();
    let vague = frame("registry.codex-acp", "", "Would you like a summary?");
    assert_eq!(baseline.state(&vague).state, PaneState::Unknown);

    let waiting = frame(
        "registry.codex-acp",
        "Action Required · codex",
        "Allow command?\npress enter to confirm or esc to cancel",
    );
    assert_eq!(baseline.state(&waiting).state, PaneState::WaitingForYou);

    let working = frame(
        "registry.codex-acp",
        "⠋ codex",
        "• Working (2s • esc to interrupt)",
    );
    assert_eq!(baseline.state(&working).state, PaneState::Working);

    let stale_blocker = frame(
        "registry.codex-acp",
        "codex",
        "Allow command?\npress enter to confirm or esc to cancel\n• Done\n›",
    );
    assert_eq!(baseline.state(&stale_blocker).state, PaneState::Idle);

    let active_title = frame("registry.codex-acp", "⠋ codex", "stale response\n›");
    assert_eq!(baseline.state(&active_title).state, PaneState::Working);

    let live_opencode_request = frame(
        "registry.opencode",
        "",
        ">\n△ Permission required\nesc dismiss · enter confirm · ↑↓ select",
    );
    assert_eq!(
        baseline.state(&live_opencode_request).state,
        PaneState::WaitingForYou
    );
}

#[test]
fn partial_or_unknown_harness_coverage_never_becomes_idle() {
    let baseline = RegexBaseline::new().unwrap();
    assert_eq!(
        baseline.state(&frame("registry.codex-acp", "", "")).state,
        PaneState::Unknown
    );
    let unknown = ObservationFrame {
        harness: HarnessIdentity::Unknown,
        ..frame("registry.codex-acp", "codex", "›")
    };
    assert_eq!(baseline.state(&unknown), Detection::unknown());
    assert_eq!(
        baseline
            .state(&frame("registry.other", "other", "> "))
            .state,
        PaneState::Unknown
    );
}

#[test]
fn unwritten_terminal_cells_do_not_split_a_real_request() {
    let baseline = RegexBaseline::new().unwrap();
    let observation = frame(
        "registry.claude-acp",
        "",
        "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc\0to cancel · Enter\0to confirm",
    );
    assert_eq!(baseline.state(&observation).state, PaneState::WaitingForYou);
}

struct AlwaysUnknown;

impl Detector for AlwaysUnknown {
    fn state(&self, _: &ObservationFrame) -> Detection {
        Detection::unknown()
    }
}

fn run(detector: &dyn Detector, observation: &ObservationFrame) -> PaneState {
    detector.state(observation).state
}

#[test]
fn detector_is_swappable_without_changing_the_caller() {
    let observation = frame("registry.opencode", "", "thinking · esc to interrupt");
    assert_eq!(
        run(&RegexBaseline::new().unwrap(), &observation),
        PaneState::Working
    );
    assert_eq!(run(&AlwaysUnknown, &observation), PaneState::Unknown);
}

#[test]
fn observations_and_labels_are_bounded_and_have_distinct_row_kinds() {
    let observation = frame("registry.codex-acp", "codex", "›");
    assert_eq!(
        RecordedRow::Observation(observation.clone()).validate(),
        Ok(())
    );
    let label = GroundTruthLabel {
        pane_id: observation.pane_id.clone(),
        pane_kind: soyeht_pane_state::ObservationPaneKind::Terminal,
        harness: observation.harness.clone(),
        timestamp_ms: observation.timestamp_ms,
        sequence: 2,
        state: PaneState::Idle,
        source: StateSource::Protocol,
        provenance: GroundTruthProvenance::protocol(1),
    };
    assert_eq!(RecordedRow::Label(label.clone()).validate(), Ok(()));
    let observation_json = serde_json::to_value(RecordedRow::Observation(observation.clone()))
        .expect("observation row serializes");
    let label_json =
        serde_json::to_value(RecordedRow::Label(label.clone())).expect("label row serializes");
    assert_eq!(observation_json["kind"], "observation");
    assert_eq!(label_json["kind"], "label");
    assert_ne!(observation_json, label_json);
    assert!(
        serde_json::from_value::<RecordedRow>(serde_json::json!({ "kind": "future" })).is_err()
    );

    let mut oversized = observation;
    oversized.lines = vec!["line".into(); 65];
    assert_eq!(oversized.validate(), Err(Error::Limit("screen lines")));
    oversized.lines = vec!["two\nlines".into()];
    assert_eq!(oversized.validate(), Err(Error::InvalidScreenLine));

    let mut empty_optional = frame("registry.codex-acp", "codex", "›");
    empty_optional.foreground_process = Some(String::new());
    assert_eq!(
        empty_optional.validate(),
        Err(Error::Empty("foreground process"))
    );

    let mut invalid_label = label.clone();
    invalid_label.state = PaneState::Unknown;
    assert_eq!(invalid_label.validate(), Err(Error::InvalidLabelState));
    invalid_label.state = PaneState::Idle;
    invalid_label.source = StateSource::Screen;
    assert_eq!(invalid_label.validate(), Err(Error::InvalidLabelSource));

    let mut claimed_hook = label.clone();
    claimed_hook.source = StateSource::Hook;
    assert_eq!(claimed_hook.validate(), Err(Error::InvalidLabelSource));
    claimed_hook.provenance = GroundTruthProvenance::session_hook(1, "a".repeat(64));
    assert_eq!(claimed_hook.validate(), Ok(()));
    claimed_hook.provenance = GroundTruthProvenance::session_hook(0, "a".repeat(64));
    assert_eq!(claimed_hook.validate(), Err(Error::InvalidLabelSource));
    claimed_hook.provenance = GroundTruthProvenance::session_hook(1, "A".repeat(64));
    assert_eq!(claimed_hook.validate(), Err(Error::InvalidLabelSource));

    let mut claimed_protocol = label;
    claimed_protocol.provenance = GroundTruthProvenance::session_hook(1, "a".repeat(64));
    assert_eq!(claimed_protocol.validate(), Err(Error::InvalidLabelSource));
    assert_eq!(
        Confidence::from_basis_points(10_001),
        Err(Error::InvalidConfidence)
    );
}
