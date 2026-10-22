//! A file the engine refuses as text is a state of the editor, never a banner.

use super::{
    EditorCommand, EditorEvent, Failure,
    load_tests::closed,
    refusal::{self, Refusal},
    tests::{doc, snapshot},
};
use crate::{
    Command, Error, Event, State,
    testing::{event, pane, pane_json, text, vector},
};
use serde_json::json;
use soyeht_protocol::{
    EventName, RpcError,
    client_policy::{EDITOR_FILE_NOT_TEXT, EDITOR_FILE_TOO_LARGE},
};

fn refused_with(kind: &str) -> Error {
    Error::Remote(
        serde_json::from_value::<RpcError>(
            json!({"code": -32039, "message": "the engine's own text", "data": {"kind": kind}}),
        )
        .unwrap(),
    )
}

fn open(path: &str) -> Command {
    Command::Editor(EditorCommand::Open {
        workspace_id: "w".into(),
        path: path.into(),
        preview: true,
        at: None,
    })
}

fn refused(path: &str, text: &'static str) -> Event {
    Event::Editor(EditorEvent::Failed(Failure::Refused {
        workspace_id: "w".into(),
        refusal: Refusal {
            path: path.into(),
            text,
        },
    }))
}

#[test]
fn where_a_failed_open_is_shown_follows_the_editor_vector() {
    for case in vector("editor")["openFailure"]["cases"].as_array().unwrap() {
        let name = text(&case["name"]);
        let error = refused_with(text(&case["kind"]));
        let command = open("blob.bin");
        match text(&case["placement"]) {
            "inline" => {
                let expected = match text(&case["text"]) {
                    "editorFileNotText" => EDITOR_FILE_NOT_TEXT,
                    "editorFileTooLarge" => EDITOR_FILE_TOO_LARGE,
                    other => panic!("{name}: unknown text {other}"),
                };
                assert!(command.failure_is_shown_in_place(&error), "{name}");
                let Some(Event::Editor(EditorEvent::Failed(Failure::Refused {
                    workspace_id,
                    refusal,
                }))) = command.failure(&error)
                else {
                    panic!("{name}: the open is not refused");
                };
                assert_eq!(workspace_id, "w", "{name}");
                assert_eq!(refusal.path, "blob.bin", "{name}");
                assert_eq!(refusal.text, expected, "{name}");
            }
            _ => {
                assert!(!command.failure_is_shown_in_place(&error), "{name}");
                assert!(
                    matches!(
                        command.failure(&error),
                        Some(Event::Editor(EditorEvent::Failed(Failure::OpenFailed(_))))
                    ),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn only_an_open_is_ever_shown_in_place() {
    let watch = Command::Editor(EditorCommand::Watch {
        workspace_id: "w".into(),
        paths: Vec::new(),
    });
    assert!(!watch.failure_is_shown_in_place(&refused_with("file_not_text")));
    assert!(refusal::of_error(&Error::Disconnected).is_none());
}

#[test]
fn a_refusal_is_held_until_a_file_opens_or_a_tab_is_chosen() {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(
        vec![pane("e1", "w", "editor")],
        vec![doc("f1", "e1", 1, 1, false)],
    )));
    state.apply(refused("blob.bin", EDITOR_FILE_NOT_TEXT));
    assert_eq!(
        state
            .editors
            .refusal("w")
            .map(|held| (held.path.as_str(), held.text)),
        Some(("blob.bin", EDITOR_FILE_NOT_TEXT))
    );
    assert!(state.editors.refusal("other").is_none());

    state.editors.activated(&pane("e1", "w", "editor"));
    assert!(state.editors.refusal("w").is_none());

    state.apply(refused("big.txt", EDITOR_FILE_TOO_LARGE));
    assert!(state.editors.refusal("w").is_some());
    let opened = serde_json::from_value(json!({
        "file": doc("f2", "e2", 1, 1, false),
        "pane": serde_json::to_value(pane("e2", "w", "editor")).unwrap(),
        "viewport": {
            "file": doc("f2", "e2", 1, 1, false), "source": "buffer", "fromLine": 0,
            "toLine": 1, "lines": [{"line": 0, "text": "x", "highlights": []}]
        }
    }))
    .unwrap();
    state.apply(Event::Editor(EditorEvent::Opened(Box::new(opened))));
    assert!(state.editors.refusal("w").is_none());
}

/// A workspace `w` whose editor holds the tab `e1` and has just refused `blob.bin`.
fn refusing() -> State {
    let mut state = super::tests::open(&["fn main() {}"], 1);
    state.apply(refused("blob.bin", EDITOR_FILE_NOT_TEXT));
    assert!(state.editors.refusal("w").is_some());
    state
}

fn created(state: &mut State, id: &str, mode: &str) {
    state.apply(event(
        EventName::PaneCreated,
        json!({"pane": pane_json(id, "w", mode)}),
    ));
}

#[test]
fn a_later_open_that_fails_for_another_reason_drops_the_refusal() {
    let mut state = refusing();
    let failed = open("gone.txt")
        .failure(&refused_with("not_found"))
        .unwrap();
    state.apply(failed);
    assert!(state.editors.refusal("w").is_none());
}

#[test]
fn an_editor_pane_that_arrives_by_event_shows_its_file_not_the_refusal() {
    let mut state = refusing();
    created(&mut state, "e2", "editor");
    assert!(state.editors.refusal("w").is_none());
}

#[test]
fn a_pane_of_another_kind_leaves_the_refusal_alone() {
    let mut state = refusing();
    created(&mut state, "t1", "terminal");
    assert!(state.editors.refusal("w").is_some());
}

#[test]
fn closing_the_last_tab_drops_the_refusal() {
    let mut state = refusing();
    closed(&mut state);
    assert!(state.editors.refusal("w").is_none());
}

#[test]
fn closing_one_of_two_tabs_keeps_the_refusal() {
    let mut state = refusing();
    created(&mut state, "e2", "editor");
    state.apply(refused("blob.bin", EDITOR_FILE_NOT_TEXT));
    closed(&mut state);
    assert!(state.editors.refusal("w").is_some());
}

#[test]
fn closing_the_workspace_drops_the_refusal() {
    let mut state = refusing();
    state.apply(event(
        EventName::WorkspaceClosed,
        json!({"workspaceId": "w"}),
    ));
    assert!(state.editors.refusal("w").is_none());
}
