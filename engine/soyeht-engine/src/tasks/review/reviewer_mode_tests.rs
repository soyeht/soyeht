use super::{ModeCeiling, State, reviewer_mode};
use crate::test_panes::agent_with_modes;

/// An executor on the harness `test` that announces `choices` and runs in `current`.
fn executor(current: &str, choices: &[(&str, &str)]) -> State {
    agent_with_modes("test", current, choices)
}

fn reviewer(executor: &State) -> Option<String> {
    reviewer_mode(executor, &ModeCeiling::of(executor, true))
}

const CLAUDE: [(&str, &str); 5] = [
    ("default", "standard"),
    ("acceptEdits", "standard"),
    ("plan", "plan"),
    ("auto", "auto_review"),
    ("bypassPermissions", "full_access"),
];
const CODEX: [(&str, &str); 3] = [
    ("read-only", "standard"),
    ("agent", "auto_review"),
    ("agent-full-access", "full_access"),
];

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
            reviewer(&executor(current, &CLAUDE)).as_deref(),
            Some("plan"),
            "an executor in {current}"
        );
    }
    // The Codex adapter announces no plan mode: its `read-only` is a standard one.
    for current in ["read-only", "agent", "agent-full-access"] {
        assert_eq!(
            reviewer(&executor(current, &CODEX)).as_deref(),
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
