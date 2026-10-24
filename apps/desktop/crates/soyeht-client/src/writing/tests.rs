//! The writing panes against the events the engine sends, through the whole `State`.

use super::{
    Command as WritingCommand, Failure, Opened, WritingEvent,
    document::position_of,
    rules::{DocumentDraft, Submission},
    screenplay::Alignment,
};
use crate::{
    Command, Effect, Event, Snapshot, State,
    editor::{EditorCommand, EditorEvent},
    testing::{event, pane_json, stored_layout, workspace},
};
use serde_json::{Value, json};
use soyeht_protocol::{
    EventName, FileReadResult, Pane, ScreenplayElementKind, TextEdit, WritingDocument,
    WritingFormatKind, WritingSection, WritingStructureResult, limits::EDITOR_EDIT_BYTES,
};

fn file(revision: u64, lines: u32, dirty: bool) -> Value {
    json!({
        "fileId": "f1", "paneId": "p1", "workspaceId": "w", "path": "texts/essay.md",
        "language": "markdown", "revision": revision.to_string(), "diskRevision": "1",
        "lineCount": lines, "byteCount": "100", "dirty": dirty, "conflicted": false,
        "watching": true
    })
}

fn document(revision: u64, lines: u32) -> WritingDocument {
    serde_json::from_value(json!({
        "file": file(revision, lines, false), "kind": "text", "section": "texts",
        "sidecarRevision": "1", "font": "reading_serif", "focusMode": false, "fullWidth": false
    }))
    .unwrap()
}

fn writing_pane() -> Pane {
    let mut pane = pane_json("p1", "w", "writing");
    pane["writingKind"] = json!("text");
    pane["writingSection"] = json!("texts");
    serde_json::from_value(pane).unwrap()
}

fn page(revision: u64, texts: &[&str]) -> Event {
    page_of(revision, texts, u32::try_from(texts.len()).unwrap())
}

/// The first lines of a document of `total` lines.
fn page_of(revision: u64, texts: &[&str], total: u32) -> Event {
    let lines: Vec<Value> = texts
        .iter()
        .enumerate()
        .map(|(at, text)| json!({"line": at, "text": text, "highlights": []}))
        .collect();
    let shown = u32::try_from(texts.len()).unwrap();
    let result: FileReadResult = serde_json::from_value(json!({
        "file": file(revision, total, false), "source": "buffer",
        "fromLine": 0, "toLine": shown, "lines": lines
    }))
    .unwrap();
    Event::Editor(EditorEvent::Page(Box::new(result)))
}

fn written(revision: u64, lines: u32) -> Event {
    Event::Editor(EditorEvent::Written {
        file_id: "f1".into(),
        result: Box::new(
            serde_json::from_value(json!({
                "file": file(revision, lines, true), "changedLines": [0]
            }))
            .unwrap(),
        ),
    })
}

/// A state with the writing pane `p1` showing `texts`, as a connection starts it.
fn open(texts: &[&str]) -> State {
    let mut state = State::default();
    let lines = u32::try_from(texts.len()).unwrap();
    state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w")],
        panes: vec![writing_pane()],
        files: vec![serde_json::from_value(file(1, lines, false)).unwrap()],
        writing: vec![document(1, lines)],
        ..Snapshot::default()
    }));
    state.apply(page(1, texts));
    state
}

/// A state connected to a workspace with no pane.
fn empty() -> State {
    let mut state = State::default();
    state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w")],
        ..Snapshot::default()
    }));
    state
}

/// Whether the effects ask the engine for the lines of a file.
fn reads(effects: &[Effect]) -> bool {
    effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::Send(Command::Editor(EditorCommand::Read(_)))
        )
    })
}

fn sent_edits(effects: &[Effect]) -> Vec<(String, Vec<TextEdit>)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Send(Command::Editor(EditorCommand::Write { base, edits, .. })) => {
                Some((base.clone(), edits.clone()))
            }
            _ => None,
        })
        .collect()
}

fn sent_writing(effects: &[Effect]) -> Vec<WritingCommand> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Send(Command::Writing(command)) => Some(command.clone()),
            _ => None,
        })
        .collect()
}

fn whole_text(state: &State) -> Option<String> {
    state.editors.file_of_pane("p1")?.whole_text()
}

#[test]
fn a_connection_gives_the_pane_its_document_and_its_text() {
    let state = open(&["# Title", "", "Some words here"]);
    let document = state.writings.of_pane("p1").unwrap();
    assert_eq!(document.doc.section, WritingSection::Texts);
    assert_eq!(
        whole_text(&state).as_deref(),
        Some("# Title\n\nSome words here")
    );
}

#[test]
fn typing_in_the_field_is_the_one_edit_the_engine_is_sent() {
    let mut state = open(&["hello", "world"]);
    let text = "hello!\nworld";
    let effects = state.writing_retype("p1", text, (6, 6)).unwrap();
    let writes = sent_edits(&effects);
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, "1");
    assert_eq!(writes[0].1[0].text, "!");
    assert_eq!(
        (
            writes[0].1[0].range.start.line,
            writes[0].1[0].range.start.column
        ),
        (0, 5)
    );
    assert_eq!(whole_text(&state).as_deref(), Some(text));
    // The next keystroke waits for the answer to the first, and goes to the revision it made.
    let second = state
        .writing_retype("p1", "hello!?\nworld", (7, 7))
        .unwrap();
    assert!(sent_edits(&second).is_empty());
    let answered = state.apply(written(2, 2));
    assert_eq!(sent_edits(&answered)[0].0, "2");
    assert_eq!(sent_edits(&answered)[0].1[0].text, "?");
}

#[test]
fn a_line_break_and_a_join_are_edits_across_lines() {
    let mut state = open(&["onetwo"]);
    let split = state.writing_retype("p1", "one\ntwo", (4, 4)).unwrap();
    let edit = &sent_edits(&split)[0].1[0];
    assert_eq!((edit.range.start.line, edit.range.start.column), (0, 3));
    assert_eq!(edit.text, "\n");
    state.apply(written(2, 2));
    let joined = state.writing_retype("p1", "onetwo", (3, 3)).unwrap();
    let edit = &sent_edits(&joined)[0].1[0];
    assert_eq!(
        (
            edit.range.start.line,
            edit.range.end.line,
            edit.text.as_str()
        ),
        (0, 1, "")
    );
}

#[test]
fn a_document_that_breaks_its_lines_with_crlf_takes_new_lines_the_same_way() {
    let mut state = open(&["one\r", "two"]);
    let effects = state.writing_retype("p1", "one\n\ntwo", (4, 4)).unwrap();
    assert_eq!(sent_edits(&effects)[0].1[0].text, "\r\n");
}

#[test]
fn text_the_field_shows_is_the_text_the_page_holds() {
    let mut state = open(&["a", "b"]);
    state.writing_retype("p1", "a\nbc\nd", (6, 6)).unwrap();
    let file = state.editors.file_of_pane("p1").unwrap();
    assert_eq!(file.whole_text().as_deref(), Some("a\nbc\nd"));
    assert_eq!(file.lines, 3);
    assert_eq!(file.selection.head, position_of("a\nbc\nd", 0, 6));
}

#[test]
fn an_edit_the_engine_would_refuse_for_its_size_is_refused_here() {
    let mut state = open(&["a"]);
    let big = "x".repeat(EDITOR_EDIT_BYTES + 1);
    let refused = state.writing_retype("p1", &big, (0, 0));
    assert_eq!(refused.unwrap_err(), crate::editor::EditorFile::TOO_LARGE);
    assert_eq!(whole_text(&state).as_deref(), Some("a"));
}

#[test]
fn a_page_that_is_not_the_whole_document_is_not_edited_as_a_string() {
    let mut state = State::default();
    state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w")],
        panes: vec![writing_pane()],
        files: vec![serde_json::from_value(file(1, 5000, false)).unwrap()],
        writing: vec![document(1, 5000)],
        ..Snapshot::default()
    }));
    state.apply(page_of(1, &["only", "the", "first"], 5000));
    assert!(whole_text(&state).is_none());
    assert_eq!(
        state.writing_retype("p1", "x", (0, 0)).unwrap_err(),
        crate::editor::EditorFile::PARTIAL
    );
}

fn bold_first_word(state: &mut State, text: &str) -> Vec<Effect> {
    state.writing_format("p1", WritingFormatKind::Bold, text, (0, 5))
}

#[test]
fn a_format_goes_to_the_revision_the_document_is_at() {
    let mut state = open(&["hello world"]);
    let effects = bold_first_word(&mut state, "hello world");
    let Some(WritingCommand::Format {
        base, range, kind, ..
    }) = sent_writing(&effects).pop()
    else {
        panic!("no format: {effects:?}");
    };
    assert_eq!(base, "1");
    assert_eq!(kind, WritingFormatKind::Bold);
    assert_eq!((range.start.column, range.end.column), (0, 5));
    // Nothing selected, nothing to format.
    assert!(
        state
            .writing_format("p1", WritingFormatKind::Bold, "hello world", (3, 3))
            .is_empty()
    );
}

#[test]
fn a_format_waits_for_the_edits_before_it() {
    let mut state = open(&["hello world"]);
    state
        .writing_retype("p1", "hello world!", (12, 12))
        .unwrap();
    let waiting = bold_first_word(&mut state, "hello world!");
    assert!(sent_writing(&waiting).is_empty());
    let answered = state.apply(written(2, 1));
    let Some(WritingCommand::Format { base, .. }) = sent_writing(&answered).pop() else {
        panic!("the format did not follow the edit: {answered:?}");
    };
    assert_eq!(base, "2");
}

#[test]
fn typing_after_a_format_was_chosen_drops_it() {
    let mut state = open(&["hello world"]);
    state
        .writing_retype("p1", "hello world!", (12, 12))
        .unwrap();
    bold_first_word(&mut state, "hello world!");
    // The text moved under the selection: the range chosen on the old text is not used.
    state
        .writing_retype("p1", "hello world!!", (13, 13))
        .unwrap();
    state.apply(written(2, 1));
    let rest = state.apply(written(3, 1));
    assert!(sent_writing(&rest).is_empty());
}

#[test]
fn a_formatted_document_is_read_again() {
    let mut state = open(&["hello"]);
    let formatted: WritingEvent =
        WritingEvent::Formatted(serde_json::from_value(file(2, 1, false)).unwrap());
    let effects = state.apply(Event::Writing(formatted));
    assert!(reads(&effects), "{effects:?}");
    state.apply(page(2, &["**hello**"]));
    assert_eq!(whole_text(&state).as_deref(), Some("**hello**"));
}

fn structure(revision: u64, words: u32) -> Event {
    structure_of(revision, words, json!([]), json!([]))
}

/// What the engine says of the document at `revision`: its words, scenes and elements.
fn structure_of(revision: u64, words: u32, scenes: Value, elements: Value) -> Event {
    Event::Writing(WritingEvent::Structure(Box::new(
        serde_json::from_value::<WritingStructureResult>(json!({
            "document": serde_json::to_value(document(revision, 1)).unwrap(),
            "slides": [], "scenes": scenes, "elements": elements, "variants": [], "beats": [],
            "wordCount": words, "truncated": false
        }))
        .unwrap(),
    )))
}

#[test]
fn the_words_are_asked_once_for_each_revision_the_text_settles_at() {
    let mut state = open(&["one two three"]);
    let asked = sent_writing(&state.writing_tick());
    assert_eq!(
        asked,
        vec![WritingCommand::Structure {
            file_id: "f1".into(),
            revision: "1".into()
        }]
    );
    // Asked, and not answered yet: not asked again.
    assert!(state.writing_tick().is_empty());
    state.apply(structure(1, 3));
    assert_eq!(state.writings.of_pane("p1").unwrap().words(), Some(3));
    assert!(state.writing_tick().is_empty());
    // Typing holds the ask back until its edits are in the engine.
    state
        .writing_retype("p1", "one two three four", (18, 18))
        .unwrap();
    assert!(state.writing_tick().is_empty());
    state.apply(written(2, 1));
    assert_eq!(sent_writing(&state.writing_tick()).len(), 1);
}

#[test]
fn a_count_that_failed_is_not_asked_again_for_the_same_revision() {
    let mut state = open(&["one"]);
    state.writing_tick();
    state.apply(Event::Writing(WritingEvent::Failed(Failure::Structure {
        file_id: "f1".into(),
    })));
    assert!(state.writing_tick().is_empty());
}

#[test]
fn a_document_another_client_made_comes_with_its_pane() {
    let mut state = empty();
    let effects = state.apply(event(
        EventName::PaneCreated,
        json!({
            "pane": serde_json::to_value(writing_pane()).unwrap(),
            "file": file(1, 1, false),
            "writingDocument": serde_json::to_value(document(1, 1)).unwrap()
        }),
    ));
    assert!(state.writings.of_pane("p1").is_some());
    assert!(reads(&effects), "{effects:?}");
}

#[test]
fn a_change_the_engine_announces_reads_the_page_when_this_replica_is_behind() {
    let mut state = open(&["hello"]);
    let behind = state.apply(event(
        EventName::WritingChanged,
        json!({"fileId": "f1", "paneId": "p1", "revision": "5", "sidecarRevision": "1"}),
    ));
    assert!(reads(&behind), "{behind:?}");
    let current = state.apply(event(
        EventName::WritingChanged,
        json!({"fileId": "f1", "paneId": "p1", "revision": "1", "sidecarRevision": "1"}),
    ));
    assert!(current.is_empty(), "{current:?}");
}

#[test]
fn closing_a_writing_pane_waits_for_its_edits() {
    let mut state = open(&["hello"]);
    state.writing_retype("p1", "hello!", (6, 6)).unwrap();
    assert!(state.close_pane_effects("p1").is_empty());
    let answered = state.apply(written(2, 1));
    assert!(
        answered
            .iter()
            .any(|effect| matches!(effect, Effect::Send(Command::ClosePane { .. }))),
        "{answered:?}"
    );
}

#[test]
fn an_agent_opened_beside_a_document_is_its_companion() {
    let mut state = open(&["hello"]);
    assert_eq!(
        state
            .expect_split(
                1,
                "p1",
                soyeht_protocol::WorkspacePaneLayoutAxis::Horizontal
            )
            .as_deref(),
        Some("w")
    );
    state.apply(event(
        EventName::PaneCreated,
        json!({"pane": pane_json("g", "w", "agent")}),
    ));
    let effects = state.apply(Event::Opened {
        ticket: 1,
        pane_id: "g".into(),
    });
    let stored = stored_layout(&effects).expect("the layout is stored");
    let soyeht_protocol::WorkspacePaneLayoutNode::V1(pair) = stored else {
        panic!("not a split");
    };
    let companion = pair.companion.expect("the split carries the companion");
    assert_eq!(
        (
            companion.writing_pane_id.as_str(),
            companion.agent_pane_id.as_str()
        ),
        ("p1", "g")
    );
    // An agent split off a terminal is no companion.
    let mut state = open(&["hello"]);
    state.apply(event(
        EventName::PaneCreated,
        json!({"pane": pane_json("t", "w", "terminal")}),
    ));
    state.expect_split(2, "t", soyeht_protocol::WorkspacePaneLayoutAxis::Horizontal);
    state.apply(event(
        EventName::PaneCreated,
        json!({"pane": pane_json("g2", "w", "agent")}),
    ));
    let effects = state.apply(Event::Opened {
        ticket: 2,
        pane_id: "g2".into(),
    });
    let paired = effects.iter().any(|effect| {
        matches!(effect, Effect::StoreLayout { layout, .. }
            if matches!(&layout.tree, Some(soyeht_protocol::WorkspacePaneLayoutNode::V1(split)) if split.companion.is_some()))
    });
    assert!(!paired);
}

#[test]
fn a_format_waits_for_edits_that_then_fail_and_is_dropped() {
    let mut state = open(&["alpha beta"]);
    state.writing_retype("p1", "alpha beta?", (11, 11)).unwrap();
    assert!(sent_writing(&bold_first_word(&mut state, "alpha beta?")).is_empty());
    // The write fails: the text the range was chosen on is gone, so the format is not sent.
    let failed = state.apply(Event::Editor(EditorEvent::Failed(
        crate::editor::Failure::Edit("f1".into()),
    )));
    assert!(sent_writing(&failed).is_empty(), "{failed:?}");
    assert!(reads(&failed));
}

#[test]
fn a_lone_carriage_return_makes_the_text_not_editable() {
    let mut state = open(&["a\rb", "second"]);
    assert!(whole_text(&state).is_none());
    assert_eq!(
        state.editors.file_of_pane("p1").unwrap().why_not_editable(),
        Some(crate::editor::EditorFile::LONE_CR)
    );
    assert_eq!(
        state
            .writing_retype("p1", "a\nb\nsecond", (0, 0))
            .unwrap_err(),
        crate::editor::EditorFile::LONE_CR
    );
    // The line break of a CRLF file is not a lone one.
    let state = open(&["a\r", "b"]);
    assert_eq!(whole_text(&state).as_deref(), Some("a\nb"));
}

#[test]
fn a_text_that_is_asked_for_a_companion_has_one_and_is_not_asked_again() {
    let mut state = open(&["hello"]);
    assert!(!state.companion_pending("p1"));
    state.expect_split(
        1,
        "p1",
        soyeht_protocol::WorkspacePaneLayoutAxis::Horizontal,
    );
    assert!(state.companion_pending("p1"));
    state.apply(event(
        EventName::PaneCreated,
        json!({"pane": pane_json("g", "w", "agent")}),
    ));
    state.apply(Event::Opened {
        ticket: 1,
        pane_id: "g".into(),
    });
    assert!(!state.companion_pending("p1"));
    assert_eq!(state.companion_of("p1").as_deref(), Some("g"));
    assert_eq!(state.companion_of("g"), None);
}

mod listing;
mod presentation;
mod screenplay;
