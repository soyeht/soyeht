//! `agent.message` into a pane in full access.

use super::*;

/// What `agent.message` from the pane `chief` to a pane of `harness` in `mode` answers.
fn message_error(harness: &str, mode: &str) -> Option<ErrorKind> {
    let mut rig = Rig::new("claude", "default");
    announce(&mut rig.core, &rig.directory, "target", harness, mode);
    let sender = rig.connect(Role::Agent, Some("chief"));
    let params = json!({"targetPaneId":"target","text":"hello","idempotencyKey":"message"});
    rig.core
        .agent_peer_method(&sender, method::AGENT_MESSAGE, &params)
        .err()
        .map(|error| errors::kind(&error))
}

#[tokio::test]
async fn an_agent_cannot_send_a_message_into_a_pane_in_full_access() {
    for (harness, mode) in [
        ("claude", "bypassPermissions"),
        ("codex", "agent-full-access"),
    ] {
        assert_eq!(
            message_error(harness, mode),
            Some(ErrorKind::ForbiddenRole),
            "{mode}"
        );
    }
    for (harness, mode) in [
        ("claude", "plan"),
        ("claude", "auto"),
        ("codex", "read-only"),
    ] {
        assert_eq!(message_error(harness, mode), None, "{mode}");
    }
}

#[tokio::test]
async fn a_pane_asked_to_start_in_full_access_takes_no_message_before_its_modes_are_announced() {
    let mut rig = Rig::new("claude", "default");
    insert_agent_pane(&mut rig.core, &rig.directory, "target", WORKSPACE);
    if let Some(running) = rig.core.agents.get_mut("target") {
        running.state.mode = Some("allow_all".into());
    }
    let sender = rig.connect(Role::Agent, Some("chief"));
    let params = json!({"targetPaneId":"target","text":"hello","idempotencyKey":"message"});
    let error = rig
        .core
        .agent_peer_method(&sender, method::AGENT_MESSAGE, &params)
        .err()
        .map(|error| errors::kind(&error));
    assert_eq!(error, Some(ErrorKind::ForbiddenRole));
}
