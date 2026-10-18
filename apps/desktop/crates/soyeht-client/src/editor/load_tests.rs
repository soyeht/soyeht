//! An editor pane always ends in a page or in a reason, and its Retry asks again.

use super::{
    EditorCommand, EditorEvent, Failure, PaneLoad,
    commands::PageRequest,
    tests::{read, sent, snapshot},
};
use crate::{
    Effect, Event, State,
    testing::{event, pane_json},
};
use serde_json::json;
use soyeht_protocol::EventName;

fn connected() -> State {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(Vec::new(), Vec::new())));
    state
}

/// The engine says an editor pane `e1` exists, with no file attached.
fn created(state: &mut State) -> Vec<Effect> {
    state.apply(event(
        EventName::PaneCreated,
        json!({"pane": pane_json("e1", "w", "editor")}),
    ))
}

fn closed(state: &mut State) {
    state.apply(event(
        EventName::PaneClosed,
        json!({"paneId": "e1", "workspaceId": "w"}),
    ));
}

/// The engine's tab list of workspace `w`, with `e1` showing the file `f1` when `listed`.
fn tabs(listed: bool) -> Event {
    let tabs = if listed {
        json!([{"paneId": "e1", "fileId": "f1", "path": "src/f1.rs", "preview": false}])
    } else {
        json!([])
    };
    Event::Editor(EditorEvent::Tabs(
        serde_json::from_value(json!({"workspaceId": "w", "tabs": tabs})).unwrap(),
    ))
}

fn read_failed(state: &mut State, reason: &str) -> Vec<Effect> {
    state.apply(Event::Editor(EditorEvent::Failed(Failure::Read {
        file_id: "f1".into(),
        from_line: 0,
        reason: reason.into(),
    })))
}

fn first_page() -> EditorCommand {
    EditorCommand::Read(PageRequest {
        file_id: "f1".into(),
        from_line: 0,
        lines: super::PAGE_LINES,
    })
}

fn tabs_request() -> EditorCommand {
    EditorCommand::Tabs {
        workspace_id: "w".into(),
    }
}

/// The engine sends the first page of `f1`.
fn page_arrives(state: &mut State) {
    state.apply(read("f1", "e1", 1, 0, &["fn main() {}"], 1));
}

/// The engine lists `e1`'s file and sends its first page, as it does for a healthy pane.
fn listed_and_read(state: &mut State) {
    state.apply(tabs(true));
    page_arrives(state);
}

fn load(state: &State) -> PaneLoad {
    state.editors.pane_load("e1")
}

#[test]
fn a_pane_created_without_a_file_finds_it_in_the_tab_list() {
    let mut state = connected();
    assert_eq!(sent(&created(&mut state)), [tabs_request()]);
    assert_eq!(load(&state), PaneLoad::Loading);
    assert_eq!(sent(&state.apply(tabs(true))), [first_page()]);
    assert_eq!(load(&state), PaneLoad::Loading);
    page_arrives(&mut state);
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn a_pane_the_tab_list_leaves_out_fails_and_asks_again_on_retry() {
    let mut state = connected();
    created(&mut state);
    state.apply(tabs(false));
    let PaneLoad::Failed(reason) = load(&state) else {
        panic!("not failed: {:?}", load(&state));
    };
    assert!(reason.contains("no file"), "{reason}");
    // The list was asked for once already on this connection: Retry asks again.
    assert_eq!(sent(&state.editor_retry("e1")), [tabs_request()]);
    assert_eq!(load(&state), PaneLoad::Loading);
    listed_and_read(&mut state);
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn a_tab_list_that_cannot_be_read_fails_the_pane_and_retry_succeeds() {
    let mut state = connected();
    created(&mut state);
    state.apply(Event::Editor(EditorEvent::Failed(Failure::Tabs {
        workspace_id: "w".into(),
        reason: "The engine did not answer editor.tabs.list in time.".into(),
    })));
    assert!(matches!(load(&state), PaneLoad::Failed(_)));
    assert_eq!(sent(&state.editor_retry("e1")), [tabs_request()]);
    listed_and_read(&mut state);
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn a_failed_first_read_ends_in_failed_and_succeeds_on_retry() {
    let mut state = connected();
    created(&mut state);
    state.apply(tabs(true));
    read_failed(&mut state, "The file is unreadable.");
    assert_eq!(
        load(&state),
        PaneLoad::Failed("The file is unreadable.".into())
    );
    // The same read, asked again.
    assert_eq!(sent(&state.editor_retry("e1")), [first_page()]);
    assert_eq!(load(&state), PaneLoad::Loading);
    // A second failure is a state again, not a spinner.
    read_failed(&mut state, "Still unreadable.");
    assert_eq!(load(&state), PaneLoad::Failed("Still unreadable.".into()));
    state.editor_retry("e1");
    page_arrives(&mut state);
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn a_failed_first_read_of_a_file_this_client_holds_is_read_again_on_retry() {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(
        vec![crate::testing::pane("e1", "w", "editor")],
        vec![super::tests::doc("f1", "e1", 1, 3, false)],
    )));
    read_failed(&mut state, "The file is unreadable.");
    assert!(matches!(load(&state), PaneLoad::Failed(_)));
    assert_eq!(sent(&state.editor_retry("e1")), [first_page()]);
    state.apply(read("f1", "e1", 1, 0, &["a", "b", "c"], 3));
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn a_refresh_that_fails_keeps_the_page_on_screen() {
    let mut state = connected();
    created(&mut state);
    listed_and_read(&mut state);
    read_failed(&mut state, "The engine is busy.");
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn results_that_arrive_after_the_pane_closed_leave_nothing_behind() {
    let mut state = connected();
    created(&mut state);
    closed(&mut state);
    // The tab list asked before the close lists the pane's file: nothing is read for it.
    let late = sent(&state.apply(tabs(true)));
    assert!(late.is_empty(), "{late:?}");
    read_failed(&mut state, "The file is unreadable.");
    state.apply(Event::Editor(EditorEvent::Failed(Failure::Tabs {
        workspace_id: "w".into(),
        reason: "late".into(),
    })));
    state.apply(read("f1", "e1", 1, 0, &["a"], 1));
    assert!(state.editors.files.is_empty());
    assert!(state.editors.failures.is_empty());
    assert_eq!(load(&state), PaneLoad::Loading);
    // The pane is gone, so a Retry has nothing to ask.
    assert!(state.editor_retry("e1").is_empty());
}
