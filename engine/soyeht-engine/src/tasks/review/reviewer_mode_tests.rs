use super::{ModeCeiling, State, reviewer_mode};
use crate::test_panes::{CLAUDE_MODES, CODEX_MODES, agent_with_modes};

/// An executor on the harness `test` that announces `choices` and runs in `current`.
fn executor(current: &str, choices: &[(&str, &str)]) -> State {
    agent_with_modes("test", current, choices)
}

fn reviewer(executor: &State) -> Option<String> {
    reviewer_mode(executor, &ModeCeiling::of(executor, true))
}

#[test]
fn reviewers_prefer_plan_then_standard_and_never_full_access() {
    for current in [
        "default",
        "acceptEdits",
        "plan",
        "auto",
        "bypassPermissions",
    ] {
        assert_eq!(
            reviewer(&executor(current, &CLAUDE_MODES)).as_deref(),
            Some("plan"),
            "an executor in {current}"
        );
    }
    // The Codex adapter announces no plan mode: its `read-only` is a standard one.
    for current in ["read-only", "agent", "agent-full-access"] {
        assert_eq!(
            reviewer(&executor(current, &CODEX_MODES)).as_deref(),
            Some("read-only"),
            "an executor in {current}"
        );
    }
    let open = [("everything", "full_access"), ("auto", "auto_review")];
    assert_eq!(reviewer(&executor("everything", &open)), None);
}

#[test]
fn a_reviewer_is_never_in_a_mode_above_its_executors() {
    // A harness whose lowest mode is a standard one leaves a reviewer for an executor it cannot
    // rank no mode at all.
    let unranked = [("mystery", "unknown"), ("default", "standard")];
    assert_eq!(reviewer(&executor("mystery", &unranked)), None);
    assert_eq!(
        reviewer(&executor("default", &unranked)).as_deref(),
        Some("default")
    );
    let planned = [("plan", "plan"), ("default", "standard")];
    let ceiling = ModeCeiling::of(&executor("plan", &planned), true);
    assert!(!ceiling.allows("test", "default", Some("standard")));
    assert!(ceiling.allows("test", "plan", Some("plan")));
}
