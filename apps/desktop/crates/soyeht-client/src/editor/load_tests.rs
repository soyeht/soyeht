//! An editor pane always ends in a page or in a reason, and its Retry asks again.

use super::{
    EditorCommand, EditorEvent, Failure, PaneLoad,
    commands::PageRequest,
    tests::{read, sent, snapshot},
};
use crate::{
    Command, Effect, Event, State,
    testing::{event, pane, pane_json, text, vector},
};
use serde_json::{Value, json};
use soyeht_protocol::{EventName, client_policy::EDITOR_NO_FILE, method};
use std::collections::VecDeque;

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

/// The engine's tab list of workspace `w`, with `e1` showing `file` when it names one.
fn tabs_of(file: Option<&str>) -> Event {
    let tabs: Vec<Value> = file
        .into_iter()
        .map(|file| json!({"paneId": "e1", "fileId": file, "path": "src/f1.rs", "preview": false}))
        .collect();
    Event::Editor(EditorEvent::Tabs(
        serde_json::from_value(json!({"workspaceId": "w", "tabs": tabs})).unwrap(),
    ))
}

/// The engine's tab list of workspace `w`, with `e1` showing the file `f1` when `listed`.
fn tabs(listed: bool) -> Event {
    tabs_of(listed.then_some("f1"))
}

/// The read of `f1` that an ask numbered `attempt` made failed, or a read that is not an ask
/// for the first page when `attempt` is 0.
fn read_failed_by(state: &mut State, attempt: u64, reason: &str) -> Vec<Effect> {
    state.apply(Event::Editor(EditorEvent::Failed(Failure::Read {
        file_id: "f1".into(),
        from_line: 0,
        attempt,
        reason: reason.into(),
    })))
}

fn read_failed(state: &mut State, reason: &str) -> Vec<Effect> {
    read_failed_by(state, 0, reason)
}

/// The first page of `file`, as the ask numbered `attempt` reads it.
fn first_page_of(file: &str, attempt: u64) -> EditorCommand {
    EditorCommand::Read(PageRequest {
        file_id: file.into(),
        from_line: 0,
        lines: super::PAGE_LINES,
        attempt,
    })
}

fn first_page(attempt: u64) -> EditorCommand {
    first_page_of("f1", attempt)
}

fn tabs_request(attempt: u64) -> EditorCommand {
    EditorCommand::Tabs {
        workspace_id: "w".into(),
        attempt,
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
    assert_eq!(sent(&created(&mut state)), [tabs_request(1)]);
    assert_eq!(load(&state), PaneLoad::Loading);
    assert_eq!(sent(&state.apply(tabs(true))), [first_page(1)]);
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
    assert_eq!(reason, EDITOR_NO_FILE);
    // The list was asked for once already on this connection: Retry asks again.
    assert_eq!(sent(&state.editor_retry("e1")), [tabs_request(2)]);
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
        attempt: 1,
        reason: "The engine did not answer editor.tabs.list in time.".into(),
    })));
    assert!(matches!(load(&state), PaneLoad::Failed(_)));
    assert_eq!(sent(&state.editor_retry("e1")), [tabs_request(2)]);
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
    // The tab list is asked again, then the file it names.
    assert_eq!(sent(&state.editor_retry("e1")), [tabs_request(2)]);
    assert_eq!(load(&state), PaneLoad::Loading);
    assert_eq!(sent(&state.apply(tabs(true))), [first_page(2)]);
    // A second failure is a state again, not a spinner.
    read_failed_by(&mut state, 2, "Still unreadable.");
    assert_eq!(load(&state), PaneLoad::Failed("Still unreadable.".into()));
    state.editor_retry("e1");
    state.apply(tabs(true));
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
    assert_eq!(sent(&state.editor_retry("e1")), [first_page(1)]);
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
        attempt: 1,
        reason: "late".into(),
    })));
    state.apply(read("f1", "e1", 1, 0, &["a"], 1));
    assert!(state.editors.files.is_empty());
    assert!(state.editors.failures.is_empty());
    assert_eq!(load(&state), PaneLoad::Loading);
    // The pane is gone, so a Retry has nothing to ask.
    assert!(state.editor_retry("e1").is_empty());
}

#[test]
fn a_retry_lists_the_tabs_again_so_a_stale_file_id_cannot_fail_forever() {
    let mut state = connected();
    created(&mut state);
    state.apply(tabs(true));
    read_failed_by(&mut state, 1, "The file is gone.");
    assert!(matches!(load(&state), PaneLoad::Failed(_)));
    // The cached list still names `f1`, but Retry asks for the list again.
    assert_eq!(sent(&state.editor_retry("e1")), [tabs_request(2)]);
    assert_eq!(
        sent(&state.apply(tabs_of(Some("f2")))),
        [first_page_of("f2", 1)]
    );
    state.apply(read("f2", "e1", 1, 0, &["fn main() {}"], 1));
    assert_eq!(load(&state), PaneLoad::Ready);
}

#[test]
fn a_late_failure_of_a_superseded_ask_does_not_show_over_the_retry() {
    let mut state = connected();
    created(&mut state);
    state.apply(tabs(true));
    read_failed_by(&mut state, 1, "The file is unreadable.");
    state.editor_retry("e1");
    state.apply(tabs(true));
    // The retry's read is on its way (ask 2). An answer to the first one arrives late.
    read_failed_by(&mut state, 1, "Late.");
    assert_eq!(load(&state), PaneLoad::Loading);
    read_failed_by(&mut state, 2, "Still unreadable.");
    assert_eq!(load(&state), PaneLoad::Failed("Still unreadable.".into()));
    // The same goes for a tab list that a Retry asked again.
    state.editor_retry("e1");
    state.apply(Event::Editor(EditorEvent::Failed(Failure::Tabs {
        workspace_id: "w".into(),
        attempt: 2,
        reason: "Late.".into(),
    })));
    assert_eq!(load(&state), PaneLoad::Loading);
}

/// The engine answers every request of `effects`, and what its answers lead to, as `engine` of a
/// step of the vector says. Returns the requests it received.
fn engine_answers(state: &mut State, effects: Vec<Effect>, engine: &Value) -> Vec<String> {
    let mut requests = Vec::new();
    let mut queue = VecDeque::from(effects);
    while let Some(effect) = queue.pop_front() {
        let Effect::Send(Command::Editor(command)) = effect else {
            continue;
        };
        let refused = |what: &str| engine[what] == "fails";
        let answer = match &command {
            EditorCommand::Tabs { .. } => {
                requests.push(method::EDITOR_TABS_LIST.to_owned());
                if refused("tabs") {
                    super::commands::failure(&command, "The engine did not answer.")
                } else if engine["tabs"] == "omits" {
                    Some(tabs_of(None))
                } else {
                    Some(tabs_of(Some(engine["file"].as_str().unwrap_or("file-1"))))
                }
            }
            EditorCommand::Read(page) => {
                requests.push(format!("{}:{}", method::FILE_READ, page.file_id));
                if refused("read") {
                    super::commands::failure(&command, "The engine did not answer.")
                } else {
                    Some(read(&page.file_id, "e1", 1, 0, &["fn main() {}"], 1))
                }
            }
            _ => None,
        };
        queue.extend(answer.map(|answer| state.apply(answer)).unwrap_or_default());
    }
    requests
}

fn state_name(load: &PaneLoad) -> &'static str {
    match load {
        PaneLoad::Loading => "loading",
        PaneLoad::Ready => "ready",
        PaneLoad::Failed(_) => "failed",
    }
}

#[test]
fn the_load_states_and_their_transitions_follow_the_editor_vector() {
    for case in vector("editor")["paneLoad"]["cases"].as_array().unwrap() {
        let name = text(&case["name"]);
        let mut state = State::default();
        let mut first = if case["fileHeld"] == true {
            state.apply(Event::Connected(snapshot(
                vec![pane("e1", "w", "editor")],
                vec![super::tests::doc("file-1", "e1", 1, 1, false)],
            )))
        } else {
            state.apply(Event::Connected(snapshot(Vec::new(), Vec::new())));
            created(&mut state)
        };
        assert_eq!(state_name(&load(&state)), text(&case["initial"]), "{name}");
        for step in case["steps"].as_array().unwrap() {
            let effects = match text(&step["do"]) {
                "retry" => state.editor_retry("e1"),
                "refresh" => state.apply(event(
                    EventName::FileChangedOnDisk,
                    json!({"fileId": "file-1", "diskRevision": "2", "changedLines": []}),
                )),
                _ => std::mem::take(&mut first),
            };
            let requests = engine_answers(&mut state, effects, &step["engine"]);
            let now = load(&state);
            assert_eq!(state_name(&now), text(&step["state"]), "{name}");
            if let Some(expected) = step["requests"].as_array() {
                let expected: Vec<&str> = expected.iter().map(text).collect();
                assert_eq!(requests, expected, "{name}");
            }
            if let PaneLoad::Failed(reason) = now {
                match step["reason"].as_str() {
                    Some("noFile") => assert_eq!(reason, EDITOR_NO_FILE, "{name}"),
                    _ => assert!(!reason.is_empty(), "{name}"),
                }
            }
        }
    }
}
