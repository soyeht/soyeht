//! The modes the Codex adapter announces to this client (adapter 2.6.0), ranked for task dispatch.

use super::*;

#[test]
fn the_codex_modes_rank_by_the_kind_the_adapter_announces_for_this_client() {
    let all = [
        "read-only",
        "workspace-write",
        "agent",
        "agent-full-access",
        "permission-profile:work",
    ];
    // (the chief's mode, the modes it may start: below its rank, or the very same mode)
    let cases: [(&str, &[&str]); 4] = [
        ("read-only", &["read-only"]),
        ("workspace-write", &["workspace-write"]),
        ("agent", &["read-only", "workspace-write", "agent"]),
        // Full access is a person's decision. A mode with no kind is never shown to be below
        // anything, so a permission profile of the person's is started only by a chief that is
        // in full access already (as for any unranked mode).
        (
            "agent-full-access",
            &[
                "read-only",
                "workspace-write",
                "agent",
                "permission-profile:work",
            ],
        ),
    ];
    for (mode, allowed) in cases {
        let rig = Rig::new("codex", mode);
        for candidate in all {
            assert_eq!(
                rig.allows("codex", candidate),
                allowed.contains(&candidate),
                "a Codex chief in {mode} starting {candidate}"
            );
        }
    }
    // A chief that runs in a permission profile has no rank, so it is the lowest.
    let rig = Rig::new("codex", "permission-profile:work");
    assert!(all.iter().all(|candidate| !rig.allows("codex", candidate)));
}
