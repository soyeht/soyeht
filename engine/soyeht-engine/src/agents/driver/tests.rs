use super::*;
use crate::ports::testing::{FixedClock, MemoryStore};
use soyeht_protocol::{HarnessLaunch, PromptOrigin};

#[test]
fn rejected_permission_is_visible_without_echoing_native_payload() {
    let mut state = super::super::state::State::new(
        HarnessLaunch {
            id: "test".into(),
            name: "Test".into(),
            command: "/test".into(),
            args: Vec::new(),
            version: "1".into(),
            environment: None,
        },
        "digest".into(),
        None,
    )
    .unwrap();
    let mut changes = Changes::default();
    reject_question(
        "pane",
        &mut state,
        1,
        json!("native-id"),
        "session/request_permission",
        errors::invalid("Executable permission has no command to display"),
        &mut changes,
    )
    .unwrap();
    assert_eq!(changes.messages.len(), 1);
    assert_eq!(changes.messages[0].role, TranscriptRole::System);
    assert_eq!(changes.messages[0].kind, TranscriptKind::System);
    assert!(changes.messages[0].text.contains("no command to display"));
    assert!(changes.messages[0].text.contains("Nothing was approved"));
    assert_eq!(changes.events.len(), 1);
    assert_eq!(changes.events[0].0, "pane.update");
    assert_eq!(changes.events[0].1["kind"], "system_message");
    // The update names its text and that it is complete, as the contract requires.
    assert!(
        changes.events[0].1["text"]
            .as_str()
            .is_some_and(|text| text.contains("Nothing was approved"))
    );
    assert_eq!(changes.events[0].1["complete"], true);
    assert_eq!(changes.outgoing.len(), 1);
    assert!(matches!(
        &changes.outgoing[0],
        Message::Response { result: Err(error), .. } if error.code == -32602
    ));
}

#[test]
fn agent_session_rule_reads_requests_and_time_through_ports() {
    let store = MemoryStore::default();
    let mut state = super::super::idle::tests::ready_state();
    state
        .enqueue("do work".into(), PromptOrigin::Automatic, "fixed".into())
        .unwrap();
    state.draft_until_ms = 20_001;

    let (_, held) = next_prompt(&store, &FixedClock(20_000), "pane", 1, &mut state).unwrap();
    assert!(held.is_none());
    let (_, released) = next_prompt(&store, &FixedClock(20_001), "pane", 1, &mut state).unwrap();
    assert_eq!(released.unwrap().text, "do work");
}
