use super::*;
use crate::{
    Event,
    snapshot::Snapshot,
    testing::{pane, pane_json, workspace, workspace_json},
};
use serde_json::json;

fn connected(panes: &[(&str, &str)]) -> (State, Vec<Effect>) {
    let mut state = State::default();
    let effects = state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w")],
        panes: panes.iter().map(|(id, mode)| pane(id, "w", mode)).collect(),
        ..Snapshot::default()
    }));
    (state, effects)
}

fn notification(name: EventName, params: Value) -> Event {
    Event::Notification { name, params }
}

#[test]
fn the_badges_count_requests_waiting_and_agents_working_per_workspace() {
    let (mut state, _) = connected(&[("a", "agent"), ("b", "agent"), ("t", "terminal")]);
    assert_eq!(state.waiting_requests(None), 0);
    assert!(!state.has_working_agent(None));
    state.agents.get_mut("a").unwrap().permission = serde_json::from_value(json!({
        "requestId": "r", "paneId": "a", "generation": 1, "promptId": "p", "title": "t",
        "detail": "", "options": [], "expiresAt": "2030-01-01T00:00:00Z"
    }))
    .ok();
    state
        .panes
        .iter_mut()
        .find(|p| p.pane_id == "b")
        .unwrap()
        .state = PaneState::Working;
    assert_eq!(state.waiting_requests(None), 1);
    assert_eq!(state.waiting_requests(Some("w")), 1);
    assert_eq!(state.waiting_requests(Some("other")), 0);
    assert!(state.has_working_agent(Some("w")) && !state.has_working_agent(Some("other")));
    state.apply(Event::Disconnected("gone".into()));
    assert_eq!(state.waiting_requests(None), 0);
    assert!(!state.has_working_agent(None));
}

#[test]
fn a_snapshot_loads_every_agent_transcript_the_usage_and_the_harnesses() {
    let (state, effects) = connected(&[("a", "agent"), ("t", "terminal")]);
    assert!(state.agent("a").is_some() && state.agent("t").is_none());
    assert!(effects.contains(&Effect::LoadTranscript("a".into())));
    assert!(effects.contains(&Effect::LoadUsage) && effects.contains(&Effect::LoadHarnesses));
}

#[test]
fn a_transcript_page_replaces_the_messages_and_updates_extend_them() {
    let (mut state, _) = connected(&[("a", "agent")]);
    let item = json!({"localId": "u", "generation": 1, "seq": "1", "role": "user",
                      "kind": "message", "text": "oi", "complete": true});
    state.apply(Event::Transcript {
        pane_id: "a".into(),
        items: vec![serde_json::from_value(item).unwrap()],
        next_cursor: None,
    });
    state.apply(notification(
        EventName::PaneUpdate,
        json!({"eventId": "9", "at": "now", "paneId": "a", "kind": "agent_message_chunk",
               "generation": 1, "seq": "2", "localId": "m", "text": "Oi!", "complete": true}),
    ));
    let texts: Vec<_> = state
        .agent("a")
        .unwrap()
        .messages
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(texts, ["oi", "Oi!"]);
}

#[test]
fn a_finished_turn_reloads_the_transcript_of_that_agent_only() {
    let (mut state, _) = connected(&[("a", "agent"), ("t", "terminal")]);
    let ended = |pane: &str| {
        notification(
            EventName::PaneTurnEnded,
            json!({"eventId": "9", "at": "now", "paneId": pane, "promptId": "p",
                   "stopReason": "end_turn"}),
        )
    };
    assert_eq!(
        state.apply(ended("a")),
        [Effect::LoadTranscript("a".into())]
    );
    assert!(state.apply(ended("t")).is_empty());
}

#[test]
fn permission_requests_wait_until_they_are_answered() {
    let (mut state, _) = connected(&[("a", "agent")]);
    state.apply(notification(
        EventName::PanePermissionRequested,
        json!({"eventId": "9", "at": "now", "paneId": "a", "requestId": "r", "generation": 1,
               "promptId": "p", "title": "Run ls", "detail": "", "options": [],
               "expiresAt": "later"}),
    ));
    assert_eq!(
        state.agent("a").unwrap().permission.as_ref().unwrap().title,
        "Run ls"
    );
    state.apply(notification(
        EventName::PanePermissionAnswered,
        json!({"eventId": "10", "at": "now", "paneId": "a", "requestId": "r",
               "result": "selected", "by": "mac"}),
    ));
    assert!(state.agent("a").unwrap().permission.is_none());
}

#[test]
fn usage_readings_arrive_by_list_and_change_by_event() {
    let (mut state, _) = connected(&[]);
    let reading = |used: f64| {
        json!({"provider": "claude", "state": "ok", "source": "claude_cli_usage",
               "fetchedAt": "now",
               "primary": {"used": used, "resetsAt": "later", "label": "Weekly"}})
    };
    state.apply(Event::Usage(vec![
        serde_json::from_value(reading(0.1)).unwrap(),
    ]));
    let mut changed = reading(0.2);
    changed["eventId"] = json!("11");
    changed["at"] = json!("now");
    state.apply(notification(EventName::UsageChanged, changed));
    assert_eq!(state.usage().len(), 1);
    assert_eq!(state.usage()[0].primary.as_ref().unwrap().used, 0.2);
}

fn created(id: &str) -> Event {
    notification(
        EventName::PaneCreated,
        json!({"eventId": "9", "at": "now", "pane": pane_json(id, "w", "terminal")}),
    )
}

/// Neither a focus request nor a stored layout: no pane was settled.
fn opened_nothing(effects: &[Effect]) -> bool {
    !effects
        .iter()
        .any(|e| matches!(e, Effect::FocusPane(_) | Effect::StoreLayout { .. }))
}

fn stored(effects: &[Effect]) -> Option<WorkspacePaneLayoutNode> {
    effects.iter().find_map(|effect| match effect {
        Effect::StoreLayout { layout, .. } => layout.tree.clone(),
        _ => None,
    })
}

#[test]
fn a_split_places_the_new_pane_beside_its_anchor_and_stores_the_layout() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    assert_eq!(
        state
            .expect_split(1, "a", WorkspacePaneLayoutAxis::Vertical)
            .as_deref(),
        Some("w")
    );
    let mut effects = state.apply(created("n"));
    assert!(opened_nothing(&effects));
    effects = state.apply(Event::Opened {
        ticket: 1,
        pane_id: "n".into(),
    });
    assert!(effects.contains(&Effect::FocusPane("n".into())));
    let tree = stored(&effects).unwrap();
    assert_eq!(layout::pane_ids(&tree), ["a", "n", "b"]);
    // The request is spent: the next pane is placed by the automatic arrangement.
    let effects = state.apply(created("m"));
    assert!(opened_nothing(&effects));
    assert!(
        state
            .layout("w", |_| true)
            .is_some_and(|tree| layout::contains(&tree, "m"))
    );
}

/// What the window draws for the workspace right now, without the engine's layout event.
fn drawn(state: &State) -> WorkspacePaneLayoutNode {
    state.layout("w", |_| true).expect("a workspace with panes")
}

#[test]
fn a_split_pane_is_never_drawn_anywhere_but_beside_its_anchor() {
    for axis in [
        WorkspacePaneLayoutAxis::Horizontal,
        WorkspacePaneLayoutAxis::Vertical,
    ] {
        // The notification may come first: the pane is here, unplaced, until the answer names it.
        let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
        state.expect_split(1, "a", axis.clone());
        state.apply(created("n"));
        assert!(
            !layout::contains(&drawn(&state), "n"),
            "the new pane waits for its place instead of showing in the automatic one"
        );
        let effects = state.apply(Event::Opened {
            ticket: 1,
            pane_id: "n".into(),
        });
        let placed = stored(&effects).unwrap();
        assert_eq!(layout::pane_ids(&placed), ["a", "n", "b"]);
        // Before the engine echoes the stored layout the window already draws it.
        assert_eq!(drawn(&state), placed);

        // The answer may come first: the same place, in the same frame the pane arrives in.
        let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
        state.expect_split(2, "a", axis);
        state.apply(Event::Opened {
            ticket: 2,
            pane_id: "n".into(),
        });
        let placed = stored(&state.apply(created("n"))).unwrap();
        assert_eq!(layout::pane_ids(&placed), ["a", "n", "b"]);
        assert_eq!(drawn(&state), placed);
    }
}

#[test]
fn a_pane_someone_else_opens_while_a_split_waits_is_shown_once_the_answer_arrives() {
    let (mut state, _) = connected(&[("a", "terminal")]);
    state.expect_split(5, "a", WorkspacePaneLayoutAxis::Horizontal);
    state.apply(created("theirs"));
    assert!(!layout::contains(&drawn(&state), "theirs"));
    state.apply(Event::Opened {
        ticket: 5,
        pane_id: "n".into(),
    });
    assert!(layout::contains(&drawn(&state), "theirs"));
}

#[test]
fn the_answer_may_come_before_the_notification() {
    let (mut state, _) = connected(&[("a", "terminal")]);
    state.expect_split(2, "a", WorkspacePaneLayoutAxis::Horizontal);
    assert!(
        state
            .apply(Event::Opened {
                ticket: 2,
                pane_id: "n".into()
            })
            .is_empty()
    );
    let effects = state.apply(created("n"));
    assert!(effects.contains(&Effect::FocusPane("n".into())));
    assert_eq!(layout::pane_ids(&stored(&effects).unwrap()), ["a", "n"]);
}

#[test]
fn a_pane_someone_else_opens_is_neither_focused_nor_placed() {
    let (mut state, _) = connected(&[("a", "terminal")]);
    state.expect_split(3, "a", WorkspacePaneLayoutAxis::Vertical);
    let effects = state.apply(created("theirs"));
    assert!(opened_nothing(&effects));
    state.apply(Event::OpenFailed { ticket: 3 });
    // The failed request no longer waits: the next pane is not placed beside "a".
    let effects = state.apply(Event::Opened {
        ticket: 3,
        pane_id: "n".into(),
    });
    assert!(effects.is_empty(), "nothing waits for ticket 3 any more");
}

#[test]
fn a_connection_that_ends_drops_the_requests_in_flight() {
    let (mut state, _) = connected(&[("a", "terminal")]);
    state.expect_open(4, "w");
    state.apply(Event::Disconnected("lost".into()));
    assert!(state.expected.is_empty());
}

#[test]
fn a_stored_layout_that_cannot_be_read_is_reported() {
    let mut state = State::default();
    let mut broken = workspace_json("w");
    broken["layout"] = json!({"version": 2, "mode": "sideways"});
    state.apply(notification(
        EventName::WorkspaceCreated,
        json!({"eventId": "1", "at": "now", "workspace": broken}),
    ));
    assert!(
        state
            .problem
            .as_deref()
            .is_some_and(|p| p.contains("layout"))
    );
}

#[test]
fn a_permission_request_that_cannot_be_read_is_reported() {
    let mut state = State::default();
    state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w")],
        panes: vec![pane("a", "w", "agent")],
        requests: vec![json!({"kind": "permission", "request": {"requestId": 7}})],
        ..Snapshot::default()
    }));
    assert!(
        state
            .problem
            .as_deref()
            .is_some_and(|p| p.contains("permission"))
    );
}

#[test]
fn a_transcript_page_for_a_pane_that_is_gone_is_ignored() {
    let (mut state, _) = connected(&[("a", "agent")]);
    state.apply(Event::Transcript {
        pane_id: "gone".into(),
        items: Vec::new(),
        next_cursor: None,
    });
    assert!(state.agent("gone").is_none());
}

#[test]
fn a_layout_can_leave_out_the_panes_a_place_does_not_show() {
    let (state, _) = connected(&[("a", "terminal"), ("b", "agent")]);
    let tree = state
        .layout("w", |pane| pane.mode == PaneMode::Agent)
        .unwrap();
    assert_eq!(layout::pane_ids(&tree), ["b"]);
    assert!(state.layout("w", |_| false).is_none());
}

#[test]
fn a_workspace_layout_event_changes_how_panes_are_arranged() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    let mut changed = workspace_json("w");
    changed["layout"] = json!({"version": 2, "mode": "custom", "tree": {
        "axis": "vertical", "children": [
            {"weight": 1.0, "node": {"paneId": "b", "kind": "terminal"}},
            {"weight": 1.0, "node": {"paneId": "a", "kind": "terminal"}}]}});
    state.apply(notification(
        EventName::WorkspaceLayoutUpdated,
        json!({"eventId": "9", "at": "now", "workspace": changed}),
    ));
    let tree = state.layout("w", |_| true).unwrap();
    assert_eq!(layout::pane_ids(&tree), ["b", "a"]);
}
