use super::*;
use crate::{
    Event,
    snapshot::Snapshot,
    testing::{pane, pane_json, stored_layout as stored, workspace, workspace_json},
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
        .messages()
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
fn a_question_about_a_web_pane_waits_with_that_pane_until_it_is_answered() {
    let (mut state, _) = connected(&[("w", "web"), ("a", "agent")]);
    let asked = |pane: &str| {
        notification(
            EventName::PanePermissionRequested,
            json!({"eventId": "9", "at": "now", "paneId": pane, "requestId": "r", "generation": 1,
                   "promptId": "p", "title": "Use your sign-in", "detail": "example.com",
                   "options": [], "expiresAt": "later", "kind": "web_session",
                   "domain": "example.com"}),
        )
    };
    state.apply(asked("w"));
    assert_eq!(
        state.web_permission("w").unwrap().domain.as_deref(),
        Some("example.com")
    );
    assert!(state.web_permission("a").is_none());
    state.apply(notification(
        EventName::PanePermissionAnswered,
        json!({"eventId": "10", "at": "now", "paneId": "w", "requestId": "other",
               "result": "selected", "by": "mac"}),
    ));
    assert!(
        state.web_permission("w").is_some(),
        "another request's answer leaves it"
    );
    state.apply(notification(
        EventName::PanePermissionAnswered,
        json!({"eventId": "11", "at": "now", "paneId": "w", "requestId": "r",
               "result": "selected", "by": "mac"}),
    ));
    assert!(state.web_permission("w").is_none());
}

#[test]
fn a_question_about_a_web_pane_that_the_engine_still_waits_on_comes_back_with_a_connection() {
    let mut state = State::default();
    let request = json!({"requestId": "r", "paneId": "w", "generation": 1, "promptId": "p",
        "title": "Use your sign-in", "detail": "example.com", "options": [],
        "expiresAt": "later", "kind": "web_session", "domain": "example.com"});
    let snapshot = |requests| Snapshot {
        workspaces: vec![workspace("w")],
        panes: vec![pane("w", "w", "web")],
        requests,
        ..Snapshot::default()
    };
    state.apply(Event::Connected(snapshot(vec![
        json!({"kind": "permission", "request": request}),
    ])));
    assert_eq!(state.web_permission("w").unwrap().request_id, "r");
    // Once the engine no longer lists it, a new connection has none.
    state.apply(Event::Connected(snapshot(Vec::new())));
    assert!(state.web_permission("w").is_none());
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

/// The weights of the two panes `a` and `b` side by side, as the window draws them.
fn weights(state: &State) -> Vec<f64> {
    match drawn(state) {
        WorkspacePaneLayoutNode::V1(split) => {
            split.children.iter().map(|branch| branch.weight).collect()
        }
        WorkspacePaneLayoutNode::V0(_) => Vec::new(),
    }
}

fn moved(state: &State, share: f64) -> WorkspacePaneLayoutNode {
    let WorkspacePaneLayoutNode::V1(mut split) = drawn(state) else {
        panic!("two panes tile")
    };
    split.children[0].weight = share;
    split.children[1].weight = 1.0 - share;
    WorkspacePaneLayoutNode::V1(split)
}

/// Lets go of the dragged divider: the layout that is stored, and the workspace it is for.
fn store_draft(state: &mut State) -> (String, soyeht_protocol::WorkspacePaneLayoutEnvelope) {
    let effects = state.commit_layout_draft();
    let [
        Effect::StoreLayout {
            workspace_id,
            layout,
        },
    ] = effects.as_slice()
    else {
        panic!("one layout is stored: {effects:?}")
    };
    (workspace_id.clone(), layout.clone())
}

#[test]
fn a_divider_being_dragged_shows_its_arrangement_until_it_is_stored_or_dropped() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    let before = weights(&state);
    state.draft_layout("w", moved(&state, 0.75));
    assert_eq!(weights(&state), [0.75, 0.25]);
    state.cancel_layout_draft();
    assert_eq!(weights(&state), before);
    assert!(state.commit_layout_draft().is_empty());
}

#[test]
fn letting_go_of_a_divider_stores_the_weights_as_the_persons_arrangement() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    state.draft_layout("w", moved(&state, 0.75));
    let (workspace_id, layout) = store_draft(&mut state);
    assert_eq!(workspace_id, "w");
    assert_eq!(
        layout.mode,
        soyeht_protocol::WorkspacePaneLayoutMode::Custom
    );
    assert_eq!(
        weights(&state),
        [0.75, 0.25],
        "drawn before the engine echoes it"
    );

    // The stored layout is what a later connection gets back, so the weights survive it.
    let stored = serde_json::to_value(&layout).unwrap();
    state.apply(Event::Disconnected("gone".into()));
    let mut reconnected = State::default();
    let mut workspace = workspace("w");
    workspace.layout = stored;
    reconnected.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace],
        panes: vec![pane("a", "w", "terminal"), pane("b", "w", "terminal")],
        ..Snapshot::default()
    }));
    assert_eq!(weights(&reconnected), [0.75, 0.25]);
}

#[test]
fn a_drag_that_changes_nothing_stores_nothing() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    state.draft_layout("w", drawn(&state));
    assert!(state.commit_layout_draft().is_empty());
}

#[test]
fn a_drag_is_dropped_when_the_panes_it_arranges_are_no_longer_the_workspace_s() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    state.draft_layout("w", moved(&state, 0.75));
    state.apply(created("c"));
    assert!(layout::contains(&drawn(&state), "c"), "the new pane shows");
    assert!(state.commit_layout_draft().is_empty());
}

#[test]
fn a_layout_the_engine_refuses_is_replaced_by_the_one_it_confirmed_with_a_notice() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    let confirmed = weights(&state);
    state.draft_layout("w", moved(&state, 0.75));
    let (workspace_id, layout) = store_draft(&mut state);
    assert_eq!(weights(&state), [0.75, 0.25]);

    // The command fails: the engine's answer is an error, and the replica hears of it.
    let command = Command::SetLayout {
        workspace_id,
        layout,
    };
    let refused = command
        .failure(&crate::Error::Busy)
        .expect("a refused layout is reported");
    assert!(command.failure_is_shown_in_place(&crate::Error::Busy));
    state.apply(refused);
    assert_eq!(
        weights(&state),
        confirmed,
        "the confirmed arrangement is back"
    );
    assert_eq!(
        state.problem.as_deref(),
        Some("Couldn't save the layout. The last saved arrangement is back.")
    );
}

#[test]
fn what_the_engine_sends_is_the_layout_a_refusal_goes_back_to() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    // A first arrangement is stored and the engine echoes it: that is now the confirmed one.
    state.draft_layout("w", moved(&state, 0.75));
    state.commit_layout_draft();
    let echoed = state.workspace("w").unwrap().clone();
    state.apply(notification(
        EventName::WorkspaceLayoutUpdated,
        json!({"eventId": "7", "at": "now", "workspace": serde_json::to_value(&echoed).unwrap()}),
    ));
    state.draft_layout("w", moved(&state, 0.4));
    state.commit_layout_draft();
    assert_eq!(weights(&state), [0.4, 0.6]);
    state.apply(Event::LayoutRejected {
        workspace_id: "w".into(),
    });
    assert_eq!(weights(&state), [0.75, 0.25]);
}

/// The panes of the workspace in the order the window draws them.
fn order(state: &State) -> Vec<String> {
    layout::pane_ids(&drawn(state))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_pane_dropped_on_the_edge_of_another_goes_beside_it_and_is_stored_at_once() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal"), ("c", "terminal")]);
    let effects = state.move_pane("c", "a", layout::Zone::Left).unwrap();
    assert_eq!(
        order(&state),
        ["c", "a", "b"],
        "drawn before the engine echoes it"
    );
    let [
        Effect::StoreLayout {
            workspace_id,
            layout,
        },
    ] = effects.as_slice()
    else {
        panic!("one layout is stored: {effects:?}")
    };
    assert_eq!(workspace_id, "w");
    assert_eq!(
        layout.mode,
        soyeht_protocol::WorkspacePaneLayoutMode::Custom
    );

    // What is stored is what a later connection gets back.
    let mut reconnected = State::default();
    let mut stored = workspace("w");
    stored.layout = serde_json::to_value(layout).unwrap();
    reconnected.apply(Event::Connected(Snapshot {
        workspaces: vec![stored],
        panes: ["a", "b", "c"].map(|id| pane(id, "w", "terminal")).into(),
        ..Snapshot::default()
    }));
    assert_eq!(order(&reconnected), ["c", "a", "b"]);
}

#[test]
fn a_pane_dropped_in_the_middle_of_another_changes_places_with_it() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal"), ("c", "terminal")]);
    let effects = state.move_pane("a", "c", layout::Zone::Center).unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(order(&state), ["c", "b", "a"]);
}

#[test]
fn a_pane_dropped_on_itself_or_where_it_already_is_stores_nothing() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    let before = drawn(&state);
    assert!(
        state
            .move_pane("a", "a", layout::Zone::Left)
            .unwrap()
            .is_empty()
    );
    assert!(
        state
            .move_pane("b", "b", layout::Zone::Center)
            .unwrap()
            .is_empty()
    );
    assert_eq!(drawn(&state), before);
}

#[test]
fn a_pane_that_is_not_in_the_same_workspace_is_not_moved() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    let mut other = pane("x", "other", "terminal");
    other.workspace_id = "other".into();
    state.panes.push(other);
    let before = drawn(&state);
    for (moved, target) in [("x", "a"), ("a", "x"), ("a", "gone"), ("gone", "a")] {
        assert_eq!(
            state.move_pane(moved, target, layout::Zone::Right),
            Err(layout::Refused::NoSuchPane),
            "{moved} onto {target}"
        );
    }
    assert_eq!(drawn(&state), before);
}

#[test]
fn a_move_the_engine_refuses_is_undone_with_a_notice() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal"), ("c", "terminal")]);
    let confirmed = order(&state);
    let effects = state.move_pane("c", "a", layout::Zone::Left).unwrap();
    let [
        Effect::StoreLayout {
            workspace_id,
            layout,
        },
    ] = effects.as_slice()
    else {
        panic!("one layout is stored: {effects:?}")
    };
    let command = Command::SetLayout {
        workspace_id: workspace_id.clone(),
        layout: layout.clone(),
    };
    state.apply(command.failure(&crate::Error::Busy).expect("reported"));
    assert_eq!(
        order(&state),
        confirmed,
        "the confirmed arrangement is back"
    );
    assert_eq!(
        state.problem.as_deref(),
        Some("Couldn't save the layout. The last saved arrangement is back.")
    );
}

#[test]
fn a_move_replaces_a_divider_drag_that_was_still_showing() {
    let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
    state.draft_layout("w", moved(&state, 0.75));
    state.move_pane("b", "a", layout::Zone::Left).unwrap();
    assert_eq!(order(&state), ["b", "a"]);
    assert!(
        state.commit_layout_draft().is_empty(),
        "no draft is left over"
    );
}
