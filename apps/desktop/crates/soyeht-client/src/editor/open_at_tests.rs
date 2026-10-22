//! A file opened at a place: the caret waits there until the file is open.

use super::{
    commands::EditorCommand,
    selection::Selection,
    tests::{doc, snapshot},
};
use crate::{Command, Effect, Event, State, testing::pane};
use serde_json::json;
use soyeht_protocol::TextPosition;

/// The engine's answer to opening `src/main.rs`, shown from its first line.
fn opened() -> Box<soyeht_protocol::FileOpenResult> {
    let lines: Vec<_> = ["hello", "world", "again"]
        .iter()
        .enumerate()
        .map(|(line, text)| json!({"line": line, "text": text, "highlights": []}))
        .collect();
    let file = doc("main", "e9", 1, 3, false);
    Box::new(
        serde_json::from_value(json!({
            "file": file,
            "pane": serde_json::to_value(pane("e9", "w", "editor")).unwrap(),
            "viewport": {"file": file, "source": "buffer", "fromLine": 0, "toLine": 3, "lines": lines}
        }))
        .unwrap(),
    )
}

#[test]
fn a_file_opened_at_a_place_puts_the_caret_there_once_it_is_open() {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(Vec::new(), Vec::new())));
    let at = TextPosition { line: 1, column: 3 };
    let effects = state.editor_open_at("w", "src/main.rs", at.clone());
    let Effect::Send(Command::Editor(EditorCommand::Open { at: asked, .. })) = &effects[0] else {
        panic!("{effects:?}");
    };
    assert_eq!(asked.as_ref(), Some(&at));
    state.apply(Event::Editor(super::EditorEvent::Opened(opened())));
    let file = state.editors.file_of_pane("e9").unwrap();
    assert_eq!(file.selection.head, at);
    // The place was for that one open: the file opened again keeps its caret.
    state.editors.file_of_pane_mut("e9").unwrap().selection =
        Selection::caret(TextPosition { line: 2, column: 0 });
    state.apply(Event::Editor(super::EditorEvent::Opened(opened())));
    let file = state.editors.file_of_pane("e9").unwrap();
    assert_eq!(file.selection.head.line, 2);
}

/// Opens `src/main.rs` at `at` and answers; where the caret ends up.
fn caret_after_asking_for(at: TextPosition) -> TextPosition {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(Vec::new(), Vec::new())));
    state.editor_open_at("w", "src/main.rs", at);
    state.apply(Event::Editor(super::EditorEvent::Opened(opened())));
    state
        .editors
        .file_of_pane("e9")
        .unwrap()
        .selection
        .head
        .clone()
}

#[test]
fn a_column_past_the_end_of_its_line_puts_the_caret_at_the_end_of_the_line() {
    let at = caret_after_asking_for(TextPosition {
        line: 1,
        column: 200,
    });
    assert_eq!(at, TextPosition { line: 1, column: 5 });
}

#[test]
fn a_line_past_the_end_of_the_file_puts_the_caret_on_its_last_line() {
    let at = caret_after_asking_for(TextPosition {
        line: 9998,
        column: 0,
    });
    assert_eq!(at, TextPosition { line: 2, column: 0 });
}

#[test]
fn a_place_asked_for_before_the_connection_was_lost_is_forgotten() {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(Vec::new(), Vec::new())));
    state.editor_open_at("w", "src/main.rs", TextPosition { line: 2, column: 1 });
    state.apply(Event::Disconnected("gone".into()));
    state.apply(Event::Editor(super::EditorEvent::Opened(opened())));
    let caret = state
        .editors
        .file_of_pane("e9")
        .unwrap()
        .selection
        .head
        .clone();
    assert_eq!(caret, TextPosition { line: 0, column: 0 });
}
