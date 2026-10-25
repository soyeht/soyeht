//! Why a file could not be saved is the engine's word, and the pane keeps it until the file is
//! saved.

use super::{
    save_status::{SaveStatus, text_of},
    tests::{doc, open, saved, written},
};
use crate::testing::{event, text, vector};
use serde_json::{Value, json};
use soyeht_protocol::{
    EditorSaveFailure, EventName, FileReadResult,
    client_policy::{EDITOR_SAVE_DENIED, EDITOR_SAVE_FULL, EDITOR_SAVE_MISSING, EDITOR_SAVE_OTHER},
};

fn reason(name: &str) -> EditorSaveFailure {
    serde_json::from_value(json!(name)).unwrap()
}

fn page(read: &Value) -> FileReadResult {
    let mut page = json!({
        "file": doc("main", "e1", 1, 1, read["dirty"] == true), "source": "buffer",
        "fromLine": 0, "toLine": 0, "lines": []
    });
    if let Some(failure) = read.get("saveFailure") {
        page["saveFailure"] = failure.clone();
    }
    serde_json::from_value(page).unwrap()
}

fn shown(key: &str) -> &'static str {
    match key {
        "editorSaveMissing" => EDITOR_SAVE_MISSING,
        "editorSaveDenied" => EDITOR_SAVE_DENIED,
        "editorSaveFull" => EDITOR_SAVE_FULL,
        "editorSaveOther" => EDITOR_SAVE_OTHER,
        other => panic!("unknown text {other}"),
    }
}

#[test]
fn what_the_pane_says_of_a_failed_save_follows_the_editor_vector() {
    for case in vector("editor")["saveStatus"]["cases"].as_array().unwrap() {
        let name = text(&case["name"]);
        let mut status = SaveStatus::default();
        for step in case["steps"].as_array().unwrap() {
            if let Some(told) = step.get("told") {
                status.told(reason(text(told)));
            } else if step.get("saved").is_some() {
                status.saved();
            } else if let Some(read) = step.get("read") {
                status.read(&page(read));
            } else {
                assert!(step.get("typed").is_some(), "{name}: unknown step {step}");
            }
        }
        let expected = case["shows"].as_str().map(shown);
        assert_eq!(status.text(), expected, "{name}");
    }
}

#[test]
fn every_reason_has_its_own_text() {
    let all = [
        EditorSaveFailure::Missing,
        EditorSaveFailure::Denied,
        EditorSaveFailure::Full,
        EditorSaveFailure::Other,
    ];
    let texts: Vec<_> = all.iter().map(text_of).collect();
    for (index, one) in texts.iter().enumerate() {
        assert!(one.starts_with("Couldn't save"), "{one}");
        assert!(!texts[index + 1..].contains(one), "{one}");
    }
}

#[test]
fn the_engine_telling_a_save_failed_shows_in_the_pane_and_typing_keeps_it() {
    let mut state = open(&["let"], 1);
    assert_eq!(
        state.editors.file_of_pane("e1").unwrap().save_status.text(),
        None
    );
    state.apply(event(
        EventName::FileSaveFailed,
        json!({"fileId": "main", "reason": "missing"}),
    ));
    let file = state.editors.file_of_pane("e1").unwrap();
    assert_eq!(file.save_status.text(), Some(EDITOR_SAVE_MISSING));

    // A write the engine answered is not a save.
    state.apply(written("main", "e1", 2, 1));
    let file = state.editors.file_of_pane("e1").unwrap();
    assert_eq!(file.save_status.text(), Some(EDITOR_SAVE_MISSING));

    // The engine saved the file after all.
    state.apply(saved("main", "e1", 2, 1));
    let file = state.editors.file_of_pane("e1").unwrap();
    assert_eq!(file.save_status.text(), None);
}

#[test]
fn a_format_result_is_not_a_save_and_leaves_the_failure_alone() {
    let mut state = open(&["let"], 1);
    state.apply(event(
        EventName::FileSaveFailed,
        json!({"fileId": "main", "reason": "denied"}),
    ));
    let formatted = serde_json::from_value(doc("main", "e1", 2, 1, true)).unwrap();
    state.editors.file_changed(formatted);
    let file = state.editors.file_of_pane("e1").unwrap();
    assert_eq!(file.save_status.text(), Some(EDITOR_SAVE_DENIED));
}

#[test]
fn a_failure_told_of_a_file_this_pane_does_not_hold_is_ignored() {
    let mut state = open(&["let"], 1);
    state.apply(event(
        EventName::FileSaveFailed,
        json!({"fileId": "other", "reason": "full"}),
    ));
    assert_eq!(
        state.editors.file_of_pane("e1").unwrap().save_status.text(),
        None
    );
}
