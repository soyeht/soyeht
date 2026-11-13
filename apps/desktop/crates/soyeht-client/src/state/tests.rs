use super::*;
use crate::commands::Command;
use crate::screen::TerminalScreen;
use crate::testing::{pane, pane_json, workspace, workspace_json};
use serde_json::json;
use soyeht_protocol::{ClientFocus, ProjectTrust, ProjectTrustEntry};

fn problem_event(message: &str) -> Event {
    Event::Problem {
        cause: "internal_error".to_owned(),
        message: message.to_owned(),
    }
}

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
            Effect::LoadSupervisor,
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
fn the_engine_is_down_only_once_the_connection_is_lost() {
    let (mut state, _) = connected(across_workspaces());
    assert!(!state.engine_down());
    state.apply(Event::Disconnected("gone".into()));
    assert!(state.engine_down());
    state.apply(Event::Starting);
    assert!(!state.engine_down(), "a start is on its way");
    state.apply(Event::Connected(across_workspaces()));
    assert!(!state.engine_down());
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
    let effects = state.apply(notification(EventName::ServersLocalChanged, json!({})));
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
fn a_view_of_one_row_fits_the_least_screen_once() {
    let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
    let least = TerminalScreen::fitted(1, 1);
    assert_eq!(state.fit_terminal("t1", 1, 1), Some(least));
    assert_eq!(state.screen("t1").unwrap().size(), least);
    // The view still asks for 1 by 1 at every frame: the engine is not told again.
    assert_eq!(state.fit_terminal("t1", 1, 1), None);
    assert_eq!(state.fit_terminal("t1", 0, 0), None);
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
    assert!(state.changes_view(&problem_event("boom")));
    assert!(state.changes_view(&Event::Disconnected("lost".into())));
}

#[test]
fn a_lost_supervisor_is_said_until_the_engine_has_it_back() {
    use soyeht_protocol::EventName::{PtydLost, PtydReconnected};
    let mut state = State::default();
    state.apply(notification(PtydLost, serde_json::json!({})));
    assert_eq!(state.problem.as_deref(), Some("Supervisor disconnected"));
    state.apply(notification(PtydReconnected, serde_json::json!({})));
    assert_eq!(state.problem, None);
    // What the person was told about something else stays.
    state.apply(problem_event("boom"));
    state.apply(notification(PtydReconnected, serde_json::json!({})));
    assert_eq!(state.problem.as_deref(), Some("boom"));
}

#[test]
fn a_window_that_connects_while_the_supervisor_is_down_says_so() {
    let mut state = State::default();
    state.apply(Event::Connected(Snapshot::default()));
    state.apply(Event::Supervisor { connected: false });
    assert_eq!(state.problem.as_deref(), Some("Supervisor disconnected"));
    state.apply(Event::Supervisor { connected: true });
    assert_eq!(state.problem, None);
}

#[test]
fn problems_are_kept_until_the_next_connection() {
    let mut state = State::default();
    state.apply(problem_event("boom"));
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

#[test]
fn a_web_pane_follows_its_page_and_other_panes_do_not() {
    let (mut state, _) = connected(snapshot(
        &["w"],
        &[("t1", "w", "terminal"), ("web1", "w", "web")],
    ));
    let moved = |pane: &str| {
        notification(
            EventName::WebNavigated,
            json!({"eventId": "9", "at": "now", "paneId": pane, "navigationGeneration": "4",
                "url": "https://example.com/a", "title": "A"}),
        )
    };
    assert!(state.apply(moved("web1")).is_empty());
    let web = state.pane("web1").unwrap();
    assert_eq!(web.url.as_deref(), Some("https://example.com/a"));
    assert_eq!((web.title.as_deref(), web.generation), (Some("A"), 4));
    state.apply(moved("t1"));
    assert!(state.pane("t1").unwrap().url.is_none());
    assert!(state.problem.is_none());
}

#[test]
fn an_effect_that_only_asks_the_engine_is_its_requests_and_the_others_are_not() {
    assert_eq!(
        Effect::LoadTranscript("p".to_owned()).commands(),
        vec![
            Command::LoadQueue {
                pane_id: "p".to_owned()
            },
            Command::LoadTranscript {
                pane_id: "p".to_owned()
            },
        ]
    );
    assert_eq!(
        Effect::SubscribeTerminal("p".to_owned()).commands(),
        vec![Command::Subscribe {
            pane_id: "p".to_owned()
        }]
    );
    assert!(Effect::Resync.commands().is_empty());
    assert!(Effect::FocusPane("p".to_owned()).commands().is_empty());
}

fn with_trust(workspaces: &[&str], trust: &[(&str, ProjectTrust)]) -> Snapshot {
    Snapshot {
        trust: trust
            .iter()
            .map(|(id, trust)| ProjectTrustEntry {
                workspace_id: (*id).to_owned(),
                trust: trust.clone(),
            })
            .collect(),
        ..snapshot(workspaces, &[])
    }
}

fn trust_changed(ids: &[&str], trust: &str) -> Event {
    notification(
        EventName::ProjectTrustChanged,
        json!({"eventId": "7", "at": "now", "workspaceIds": ids, "trust": trust}),
    )
}

fn prompt_for(state: &State) -> Option<&str> {
    state
        .trust_prompt()
        .map(|workspace| workspace.workspace_id.as_str())
}

#[test]
fn the_first_undecided_workspace_that_is_selected_is_asked_about() {
    let (mut state, _) = connected(with_trust(
        &["w1", "w2"],
        &[
            ("w1", ProjectTrust::Trusted),
            ("w2", ProjectTrust::Undecided),
        ],
    ));
    assert_eq!(prompt_for(&state), None, "the selected one is trusted");
    state.select_workspace("w2");
    assert_eq!(prompt_for(&state), Some("w2"));
    state.select_workspace("w1");
    assert_eq!(prompt_for(&state), None);
}

#[test]
fn a_snapshot_without_the_trust_section_asks_nothing() {
    let (state, _) = connected(snapshot(&["w1"], &[]));
    assert_eq!(prompt_for(&state), None);
}

#[test]
fn putting_the_question_off_keeps_it_away_until_the_application_restarts() {
    let undecided = || with_trust(&["w1"], &[("w1", ProjectTrust::Undecided)]);
    let (mut state, _) = connected(undecided());
    assert_eq!(prompt_for(&state), Some("w1"));
    state.defer_trust_prompt();
    assert_eq!(prompt_for(&state), None);
    // A lost and restored connection does not ask again.
    state.apply(Event::Disconnected("gone".into()));
    state.apply(Event::Connected(undecided()));
    assert_eq!(prompt_for(&state), None);
}

/// Two workspaces over one folder, neither answered for.
fn two_undecided_workspaces() -> (State, Vec<Effect>) {
    connected(with_trust(
        &["w1", "w2"],
        &[
            ("w1", ProjectTrust::Undecided),
            ("w2", ProjectTrust::Undecided),
        ],
    ))
}

#[test]
fn the_engines_event_answers_the_question_for_every_workspace_of_the_folder() {
    let (mut state, _) = two_undecided_workspaces();
    assert!(
        state
            .apply(trust_changed(&["w1", "w2"], "trusted"))
            .is_empty()
    );
    assert_eq!(prompt_for(&state), None);
    state.select_workspace("w2");
    assert_eq!(prompt_for(&state), None);
    assert!(state.problem().is_none());
}

#[test]
fn an_event_that_is_not_a_trust_is_unreadable_and_asks_for_a_fresh_snapshot() {
    let (mut state, _) = connected(with_trust(&["w1"], &[("w1", ProjectTrust::Undecided)]));
    let effects = state.apply(trust_changed(&["w1"], "maybe"));
    assert!(matches!(effects.as_slice(), [Effect::Resync]));
}

#[test]
fn an_event_for_a_workspace_the_snapshot_did_not_list_is_kept() {
    // A workspace announced a moment before its trust: the answer arrives with the event.
    let (mut state, _) = connected(snapshot(&["w1"], &[]));
    state.apply(trust_changed(&["w1"], "undecided"));
    assert_eq!(prompt_for(&state), Some("w1"));
}

#[test]
fn the_answer_to_the_request_updates_the_trust_like_the_event_does() {
    let (mut state, _) = connected(with_trust(&["w1"], &[("w1", ProjectTrust::Undecided)]));
    state.apply(Event::ProjectTrust {
        workspace_ids: vec!["w1".to_owned()],
        trust: ProjectTrust::Restricted,
    });
    assert_eq!(prompt_for(&state), None);
}

#[test]
fn a_disconnect_forgets_the_trust_and_a_reconnect_brings_the_engines_back() {
    let (mut state, _) = connected(with_trust(&["w1"], &[("w1", ProjectTrust::Undecided)]));
    state.apply(Event::Disconnected("gone".into()));
    assert_eq!(prompt_for(&state), None);
    state.apply(Event::Connected(with_trust(
        &["w1"],
        &[("w1", ProjectTrust::Trusted)],
    )));
    assert_eq!(prompt_for(&state), None);
}

#[test]
fn a_trust_the_engine_could_not_save_is_told_in_the_policy_words() {
    let (mut state, _) = connected(with_trust(&["w1"], &[("w1", ProjectTrust::Undecided)]));
    state.apply(Event::ProjectTrustFailed);
    assert_eq!(
        state.problem(),
        Some(soyeht_protocol::client_policy::PROJECT_TRUST_FAILED)
    );
    assert_eq!(prompt_for(&state), Some("w1"), "the question stays");
}

#[test]
fn a_closed_workspace_is_no_longer_asked_about() {
    let (mut state, _) = two_undecided_workspaces();
    state.apply(notification(
        EventName::WorkspaceClosed,
        json!({"eventId": "8", "at": "now", "workspaceId": "w1"}),
    ));
    assert_eq!(prompt_for(&state), Some("w2"));
}

fn create_workspace(state: &mut State, id: &str) {
    state.apply(notification(
        EventName::WorkspaceCreated,
        json!({"eventId": "9", "at": "now", "workspace": workspace_json(id)}),
    ));
}

#[test]
fn a_workspace_created_during_the_session_is_asked_about_when_the_engine_says_undecided() {
    let (mut state, _) = connected(with_trust(&["w1"], &[("w1", ProjectTrust::Trusted)]));
    create_workspace(&mut state, "w2");
    state.apply(trust_changed(&["w2"], "undecided"));
    assert_eq!(prompt_for(&state), None, "w1 is still the selected one");
    state.select_workspace("w2");
    assert_eq!(prompt_for(&state), Some("w2"));
    state.apply(trust_changed(&["w2"], "trusted"));
    assert_eq!(prompt_for(&state), None);
}

#[test]
fn a_workspace_created_on_an_engine_with_no_trust_section_asks_nothing() {
    let (mut state, _) = connected(snapshot(&[], &[]));
    create_workspace(&mut state, "w1");
    assert_eq!(prompt_for(&state), None);
    assert_eq!(state.trust_marker(), None);
}

#[test]
fn a_restricted_or_undecided_workspace_is_marked_and_can_be_asked_about_again() {
    let (mut state, _) = connected(with_trust(
        &["w1", "w2"],
        &[
            ("w1", ProjectTrust::Restricted),
            ("w2", ProjectTrust::Trusted),
        ],
    ));
    let note = Some(soyeht_protocol::client_policy::PROJECT_TRUST_RESTRICTED_NOTE);
    assert_eq!(state.trust_marker(), note);
    assert_eq!(prompt_for(&state), None, "the person already answered");
    state.reopen_trust_prompt();
    assert_eq!(prompt_for(&state), Some("w1"));
    state.defer_trust_prompt();
    assert_eq!(prompt_for(&state), None);
    assert_eq!(state.trust_marker(), note, "the marker stays");
    state.apply(trust_changed(&["w1"], "trusted"));
    assert_eq!(state.trust_marker(), None);
    state.select_workspace("w2");
    assert_eq!(state.trust_marker(), None);
}

#[test]
fn a_question_put_off_comes_back_when_the_person_asks_for_it() {
    let (mut state, _) = connected(with_trust(&["w1"], &[("w1", ProjectTrust::Undecided)]));
    state.defer_trust_prompt();
    assert_eq!(prompt_for(&state), None);
    state.reopen_trust_prompt();
    assert_eq!(prompt_for(&state), Some("w1"));
}

/// A state with one pane, `d`, standing in for a document.
fn document_state() -> State {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(
        &["w"],
        &[("d", "w", "terminal")],
    )));
    state
}

#[test]
fn opening_an_agent_beside_a_document_follows_what_is_ready_and_what_is_on_its_way() {
    let mut state = document_state();
    assert_eq!(state.companion_step("d"), Companion::NoAgent);
    // The person is told, and the failure is not made when an agent can start.
    assert_eq!(
        state.problem(),
        Some("No agent is ready to open beside this document. Check Preferences.")
    );
    state.dismiss_problem();
    state.harnesses = vec![crate::testing::harness("claude", true)];
    assert_eq!(
        state.companion_step("d"),
        Companion::Open {
            harness: "claude".into()
        }
    );
    state.expect_split(1, "d", WorkspacePaneLayoutAxis::Horizontal);
    assert_eq!(state.companion_step("d"), Companion::Wait);
    assert_eq!(state.problem(), None);
}

#[test]
fn closing_a_document_the_replica_holds_no_file_for_needs_no_question() {
    let state = document_state();
    assert_eq!(
        state.writing_closing("d"),
        crate::pane_close::Closing::Immediately
    );
}
