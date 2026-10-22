use super::*;
use crate::testing::{pane, pane_json, workspace, workspace_json};
use serde_json::json;
use soyeht_protocol::ClientFocus;

fn snapshot(workspaces: &[&str], panes: &[(&str, &str, &str)]) -> Snapshot {
    Snapshot {
        workspaces: workspaces.iter().map(|id| workspace(id)).collect(),
        panes: panes
            .iter()
            .map(|(id, ws, mode)| pane(id, ws, mode))
            .collect(),
        focus: None,
        cursor: 1,
        ..Snapshot::default()
    }
}

/// A terminal in each of two workspaces.
fn across_workspaces() -> Snapshot {
    snapshot(
        &["w1", "w2"],
        &[("t1", "w1", "terminal"), ("t2", "w2", "terminal")],
    )
}

/// Two terminals in one workspace.
fn side_by_side() -> Snapshot {
    snapshot(&["w"], &[("t1", "w", "terminal"), ("t2", "w", "terminal")])
}

fn connected(snapshot: Snapshot) -> (State, Vec<Effect>) {
    let mut state = State::default();
    let effects = state.apply(Event::Connected(snapshot));
    (state, effects)
}

fn notification(name: EventName, params: Value) -> Event {
    Event::Notification { name, params }
}

#[test]
fn a_snapshot_lists_workspaces_and_panes_and_subscribes_every_terminal() {
    let (state, effects) = connected(snapshot(
        &["w1", "w2"],
        &[
            ("t1", "w1", "terminal"),
            ("a1", "w1", "agent"),
            ("t2", "w2", "terminal"),
        ],
    ));
    assert_eq!(state.connection, Connection::Connected);
    assert_eq!(state.workspaces().len(), 2);
    assert_eq!(state.panes_of("w1").count(), 2);
    assert_eq!(
        effects,
        [
            Effect::SubscribeTerminal("t1".into()),
            Effect::SubscribeTerminal("t2".into()),
            Effect::LoadUsage,
            Effect::LoadHarnesses,
            Effect::LoadReadiness,
            Effect::LoadTranscript("a1".into()),
            Effect::LoadCommands("a1".into()),
            Effect::LoadSkills(Some("w1".into())),
        ]
    );
    assert!(state.screen("t1").is_some() && state.screen("a1").is_none());
    assert_eq!(state.screen("t1").unwrap().size(), (24, 80));
}

#[test]
fn selection_defaults_to_the_engine_focus_then_the_first_workspace() {
    let mut shot = across_workspaces();
    shot.focus = Some(ClientFocus {
        workspace_id: Some("w2".into()),
        pane_id: Some("t2".into()),
    });
    let (state, _) = connected(shot);
    assert_eq!(state.selected_workspace().unwrap().workspace_id, "w2");
    assert_eq!(state.focused_pane().unwrap().pane_id, "t2");

    let (state, _) = connected(snapshot(&["w1", "w2"], &[("t1", "w1", "terminal")]));
    assert_eq!(state.selected_workspace().unwrap().workspace_id, "w1");
    assert_eq!(state.focused_pane().unwrap().pane_id, "t1");
}

#[test]
fn starting_an_engine_is_a_connection_state_of_its_own() {
    let (mut state, _) = connected(across_workspaces());
    state.apply(Event::Disconnected("gone".into()));
    state.apply(Event::Starting);
    assert_eq!(state.connection, Connection::Starting);
    state.apply(Event::Connected(across_workspaces()));
    assert_eq!(state.connection, Connection::Connected);
}

#[test]
fn reconnecting_keeps_the_selection_and_resubscribes() {
    let (mut state, _) = connected(across_workspaces());
    state.focus_pane("t2");
    state.apply(Event::Disconnected("lost".into()));
    assert_eq!(state.connection, Connection::Disconnected("lost".into()));
    let effects = state.apply(Event::Connected(across_workspaces()));
    assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
    assert!(effects.contains(&Effect::SubscribeTerminal("t1".into())));
    assert!(effects.contains(&Effect::SubscribeTerminal("t2".into())));
}

#[test]
fn terminal_output_reaches_only_its_pane_and_reset_clears_it() {
    let (mut state, _) = connected(side_by_side());
    state.apply(Event::Terminal(StreamOutput::Bytes {
        pane_id: "t1".into(),
        bytes: b"hi".to_vec(),
    }));
    assert!(state.screen("t1").unwrap().contents().starts_with("hi"));
    assert!(state.screen("t2").unwrap().contents().trim().is_empty());
    state.apply(Event::Terminal(StreamOutput::Reset {
        pane_id: "t1".into(),
    }));
    assert!(state.screen("t1").unwrap().contents().trim().is_empty());
}

#[test]
fn gaps_are_remembered_until_the_next_reset() {
    let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
    state.apply(Event::Terminal(StreamOutput::Gap {
        pane_id: "t1".into(),
    }));
    assert!(state.has_gap("t1"));
    state.apply(Event::Terminal(StreamOutput::Reset {
        pane_id: "t1".into(),
    }));
    assert!(!state.has_gap("t1"));
}

#[test]
fn created_panes_appear_and_new_terminals_are_subscribed() {
    let (mut state, _) = connected(snapshot(&["w"], &[]));
    let effects = state.apply(notification(
        EventName::PaneCreated,
        json!({"eventId": "2", "at": "now", "pane": pane_json("t9", "w", "terminal")}),
    ));
    assert_eq!(effects, [Effect::SubscribeTerminal("t9".into())]);
    assert_eq!(state.focused_pane().unwrap().pane_id, "t9");
    assert!(state.screen("t9").is_some());
}

#[test]
fn renamed_panes_update_in_place_without_a_second_subscription() {
    let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
    let mut renamed = pane_json("t1", "w", "terminal");
    renamed["name"] = json!("build");
    let effects = state.apply(notification(
        EventName::PaneRenamed,
        json!({"eventId": "3", "at": "now", "pane": renamed}),
    ));
    assert!(effects.is_empty());
    assert_eq!(state.panes()[0].name, "build");
    assert_eq!(state.panes().len(), 1);
}

#[test]
fn an_update_that_trails_a_close_does_not_bring_the_pane_back() {
    let (mut state, _) = connected(side_by_side());
    state.apply(notification(
        EventName::PaneClosed,
        json!({"eventId": "4", "at": "now", "paneId": "t1", "workspaceId": "w"}),
    ));
    let mut dead = pane_json("t1", "w", "terminal");
    dead["state"] = json!("dead");
    state.apply(notification(
        EventName::PaneUpdated,
        json!({"eventId": "5", "at": "now", "pane": dead}),
    ));
    assert_eq!(state.panes().len(), 1);
    assert!(state.screen("t1").is_none());
}

#[test]
fn closing_the_focused_pane_moves_focus_to_a_neighbour() {
    let (mut state, _) = connected(side_by_side());
    assert_eq!(state.focused_pane().unwrap().pane_id, "t1");
    state.apply(notification(
        EventName::PaneClosed,
        json!({"eventId": "4", "at": "now", "paneId": "t1", "workspaceId": "w"}),
    ));
    assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
    assert!(state.screen("t1").is_none());
}

#[test]
fn pane_state_changes_update_the_pane() {
    let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
    state.apply(notification(
        EventName::PaneStateChanged,
        json!({"eventId": "5", "at": "now", "paneId": "t1", "state": "working",
               "confidence": 0.9, "phrase": "Building", "source": "osc"}),
    ));
    assert_eq!(state.panes()[0].phrase, "Building");
}

#[test]
fn workspace_events_add_rename_and_close() {
    let (mut state, _) = connected(snapshot(&[], &[]));
    assert!(state.selected_workspace().is_none());
    state.apply(notification(
        EventName::WorkspaceCreated,
        json!({"eventId": "6", "at": "now", "workspace": workspace_json("w1")}),
    ));
    assert_eq!(state.selected_workspace().unwrap().workspace_id, "w1");
    let mut renamed = workspace_json("w1");
    renamed["name"] = json!("api");
    state.apply(notification(
        EventName::WorkspaceRenamed,
        json!({"eventId": "7", "at": "now", "workspace": renamed}),
    ));
    assert_eq!(state.workspaces()[0].name, "api");
    state.apply(notification(
        EventName::WorkspaceClosed,
        json!({"eventId": "8", "at": "now", "workspaceId": "w1"}),
    ));
    assert!(state.workspaces().is_empty());
    assert!(state.selected_workspace().is_none());
}

#[test]
fn unrelated_notifications_change_nothing() {
    let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
    let effects = state.apply(notification(EventName::WebNavigated, json!({})));
    assert!(effects.is_empty() && state.problem.is_none());
    assert_eq!(state.panes().len(), 1);
}

#[test]
fn an_unreadable_state_event_reports_a_problem_and_asks_for_a_fresh_snapshot() {
    for name in [
        EventName::WorkspaceCreated,
        EventName::WorkspaceClosed,
        EventName::PaneCreated,
        EventName::PaneUpdated,
        EventName::PaneClosed,
        EventName::PaneStateChanged,
    ] {
        let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
        let effects = state.apply(notification(name, json!({"nonsense": true})));
        assert_eq!(effects, [Effect::Resync], "{}", name.as_str());
        assert!(
            state
                .problem
                .as_deref()
                .is_some_and(|p| p.contains(name.as_str())),
            "{}",
            name.as_str()
        );
        assert_eq!(state.panes().len(), 1);
    }
}

#[test]
fn fitting_a_terminal_reports_only_real_size_changes() {
    let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
    assert_eq!(state.fit_terminal("t1", 24, 80), None);
    assert_eq!(state.fit_terminal("t1", 30, 100), Some((30, 100)));
    assert_eq!(state.screen("t1").unwrap().size(), (30, 100));
    assert_eq!(state.fit_terminal("missing", 30, 100), None);
}

#[test]
fn selecting_a_workspace_focuses_its_first_terminal() {
    let (mut state, _) = connected(across_workspaces());
    state.select_workspace("w2");
    assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
    state.select_workspace("missing");
    assert_eq!(state.selected_workspace().unwrap().workspace_id, "w2");
}

#[test]
fn a_workspace_that_was_not_shown_has_its_screens_when_it_is_selected() {
    let (mut state, _) = connected(across_workspaces());
    let output = |pane_id: &str| {
        Event::Terminal(StreamOutput::Bytes {
            pane_id: pane_id.into(),
            bytes: b"prompt $ ".to_vec(),
        })
    };
    // Output of the workspace shown changes the view; output of the other one does not, but its
    // replica takes it.
    assert!(state.changes_view(&output("t1")));
    assert!(!state.changes_view(&output("t2")));
    assert!(!state.changes_view(&output("missing")));
    state.apply(output("t2"));
    // Selecting needs no event and no request: the state shows the new workspace at once.
    state.select_workspace("w2");
    assert_eq!(state.selected_workspace().unwrap().workspace_id, "w2");
    assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
    assert!(
        state
            .screen("t2")
            .unwrap()
            .contents()
            .starts_with("prompt $")
    );
    assert!(state.changes_view(&output("t2")));
    assert!(!state.changes_view(&output("t1")));
}

#[test]
fn events_other_than_terminal_output_change_the_view() {
    let (state, _) = connected(across_workspaces());
    assert!(state.changes_view(&Event::Problem("boom".into())));
    assert!(state.changes_view(&Event::Disconnected("lost".into())));
}

#[test]
fn problems_are_kept_until_the_next_connection() {
    let mut state = State::default();
    state.apply(Event::Problem("boom".into()));
    assert_eq!(state.problem.as_deref(), Some("boom"));
    state.apply(Event::Connected(Snapshot::default()));
    assert_eq!(state.problem, None);
}

#[test]
fn a_connection_clears_the_problem_but_not_the_clients_standing_notice() {
    let mut state = State {
        problem: Some("A request failed.".to_owned()),
        notice: Some("Drawing on a software renderer.".to_owned()),
        ..State::default()
    };
    state.apply(Event::Connected(Snapshot::default()));
    assert!(state.problem.is_none());
    assert_eq!(
        state.notice.as_deref(),
        Some("Drawing on a software renderer.")
    );
}
