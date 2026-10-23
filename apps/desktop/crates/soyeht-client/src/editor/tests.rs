//! The editor replica against the events the engine sends, through the whole `State`.

use super::{commands::PageRequest, selection::Motion};
use crate::{
    Command, Effect, Event, Snapshot, State,
    testing::{event, pane, workspace},
};
use serde_json::{Value, json};
use soyeht_protocol::{EventName, FileReadResult, Pane, TextEdit, TextPosition};

pub(super) fn doc(file: &str, pane_id: &str, revision: u64, lines: u32, dirty: bool) -> Value {
    json!({
        "fileId": file, "paneId": pane_id, "workspaceId": "w", "path": format!("src/{file}.rs"),
        "language": "rust", "revision": revision.to_string(), "diskRevision": "1",
        "lineCount": lines, "byteCount": "100", "dirty": dirty, "conflicted": false,
        "watching": true
    })
}

pub(super) fn read(
    file: &str,
    pane_id: &str,
    revision: u64,
    from: u32,
    texts: &[&str],
    total: u32,
) -> Event {
    let lines: Vec<Value> = texts
        .iter()
        .enumerate()
        .map(|(index, text)| json!({"line": from + index as u32, "text": text, "highlights": []}))
        .collect();
    let result: FileReadResult = serde_json::from_value(json!({
        "file": doc(file, pane_id, revision, total, false), "source": "buffer",
        "fromLine": from, "toLine": from + texts.len() as u32, "lines": lines
    }))
    .unwrap();
    Event::Editor(super::EditorEvent::Page(Box::new(result)))
}

pub(super) fn written(file: &str, pane_id: &str, revision: u64, lines: u32) -> Event {
    Event::Editor(super::EditorEvent::Written {
        file_id: file.into(),
        result: Box::new(
            serde_json::from_value(json!({
                "file": doc(file, pane_id, revision, lines, true), "changedLines": [0]
            }))
            .unwrap(),
        ),
    })
}

pub(super) fn snapshot(panes: Vec<Pane>, files: Vec<Value>) -> Snapshot {
    Snapshot {
        workspaces: vec![workspace("w")],
        panes,
        files: files
            .into_iter()
            .map(|file| serde_json::from_value(file).unwrap())
            .collect(),
        ..Snapshot::default()
    }
}

/// The last 501 lines of a file of 5000, `l4499` to `l4999`, as a page that starts at line 4499.
fn read_tail() -> Event {
    let texts: Vec<String> = (4499..5000).map(|line| format!("l{line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    read("main", "e1", 1, 4499, &refs, 5000)
}

/// What the effects send to the engine, as editor commands.
pub(super) fn sent(effects: &[Effect]) -> Vec<super::EditorCommand> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Send(Command::Editor(command)) => Some(command.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn writes(effects: &[Effect]) -> Vec<(String, Vec<TextEdit>)> {
    sent(effects)
        .into_iter()
        .filter_map(|command| match command {
            super::EditorCommand::Write { base, edits, .. } => Some((base, edits)),
            _ => None,
        })
        .collect()
}

/// A state with one editor pane `e1` showing `src/main.rs`, whose page has been read.
pub(super) fn open(texts: &[&str], total: u32) -> State {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(
        vec![pane("e1", "w", "editor")],
        vec![doc("main", "e1", 1, total, false)],
    )));
    state.apply(read("main", "e1", 1, 0, texts, total));
    state
}

/// A file of 5000 lines whose first full page, 2000 lines, has been read.
fn open_full_page() -> State {
    let texts: Vec<String> = (0..2000).map(|line| format!("l{line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    open(&refs, 5000)
}

pub(super) fn page_text(state: &State) -> Vec<String> {
    state
        .editors
        .file_of_pane("e1")
        .and_then(|file| file.page.as_ref())
        .map(|page| page.lines.iter().map(|line| line.text.clone()).collect())
        .unwrap_or_default()
}

pub(super) fn caret(state: &State) -> TextPosition {
    state
        .editors
        .file_of_pane("e1")
        .unwrap()
        .selection
        .head
        .clone()
}

#[test]
fn a_new_connection_reads_the_page_of_every_open_file_and_the_tabs_of_its_workspace() {
    let mut state = State::default();
    let effects = state.apply(Event::Connected(snapshot(
        vec![pane("e1", "w", "editor"), pane("t1", "w", "terminal")],
        vec![doc("main", "e1", 1, 3, false)],
    )));
    let commands = sent(&effects);
    assert!(commands.contains(&super::EditorCommand::Tabs {
        workspace_id: "w".into(),
        attempt: 1
    }));
    let reads: Vec<_> = commands
        .iter()
        .filter(|command| matches!(command, super::EditorCommand::Read(_)))
        .collect();
    assert_eq!(reads.len(), 1, "{commands:?}");
    assert_eq!(
        *reads[0],
        super::EditorCommand::Read(PageRequest {
            file_id: "main".into(),
            from_line: 0,
            lines: super::PAGE_LINES,
            attempt: 0
        })
    );
}

#[test]
fn typing_changes_the_page_at_once_and_goes_to_the_engine_one_write_after_another() {
    let mut state = open(&["fn main() {", "}"], 2);
    state.editors.motion("e1", Motion::LineEnd, false);
    // The first keystroke is sent to the revision the file is at.
    let first = state.editors.insert("e1", "a");
    assert_eq!(page_text(&state)[0], "fn main() {a");
    let sent_first = writes(&first);
    assert_eq!(sent_first.len(), 1);
    assert_eq!(sent_first[0].0, "1");
    // The next ones wait: the engine has not answered the first.
    assert!(writes(&state.editors.insert("e1", "b")).is_empty());
    assert!(writes(&state.editors.insert("e1", "c")).is_empty());
    assert_eq!(page_text(&state)[0], "fn main() {abc");
    // Each goes to the revision the one before made.
    let second = state.apply(written("main", "e1", 2, 2));
    assert_eq!(writes(&second)[0].0, "2");
    assert_eq!(writes(&second)[0].1[0].text, "b");
    let third = state.apply(written("main", "e1", 3, 2));
    assert_eq!(writes(&third)[0].0, "3");
    let done = state.apply(written("main", "e1", 4, 2));
    assert!(writes(&done).is_empty() && sent(&done).is_empty());
    assert!(state.editors.file_of_pane("e1").unwrap().doc.dirty);
    assert_eq!(
        caret(&state),
        TextPosition {
            line: 0,
            column: 14
        }
    );
}

#[test]
fn a_page_read_before_the_last_edit_is_not_taken() {
    let mut state = open(&["one", "two"], 2);
    let sent_edit = state.editors.insert("e1", "x");
    assert_eq!(writes(&sent_edit).len(), 1);
    // A page that was on its way when the person typed describes the file without the edit.
    let stale = state.apply(read("main", "e1", 1, 0, &["one", "two"], 2));
    assert_eq!(page_text(&state)[0], "xone");
    assert!(sent(&stale).is_empty(), "{stale:?}");
    // It is read again once the edit is in the engine.
    let acknowledged = state.apply(written("main", "e1", 2, 2));
    assert!(
        sent(&acknowledged)
            .iter()
            .any(|command| matches!(command, super::EditorCommand::Read(_)))
    );
    // A page of an older revision that arrives later is dropped too.
    state.apply(read("main", "e1", 1, 0, &["one", "two"], 2));
    assert_eq!(page_text(&state)[0], "xone");
    state.apply(read("main", "e1", 2, 0, &["xone", "two"], 2));
    assert_eq!(page_text(&state)[0], "xone");
}

#[test]
fn a_refused_write_reads_the_page_again() {
    let mut state = open(&["one"], 1);
    state.editors.insert("e1", "x");
    state.editors.insert("e1", "y");
    let failed = state.apply(Event::Editor(super::EditorEvent::Failed(
        super::Failure::Edit("main".into()),
    )));
    assert!(writes(&failed).is_empty());
    assert!(
        sent(&failed)
            .iter()
            .any(|command| matches!(command, super::EditorCommand::Read(_)))
    );
    // What the engine has replaces what was typed.
    state.apply(read("main", "e1", 5, 0, &["engine"], 1));
    assert_eq!(page_text(&state), ["engine"]);
}

#[test]
fn scrolling_far_in_a_long_file_asks_for_the_page_that_leads_the_view() {
    let texts: Vec<String> = (0..2000).map(|line| format!("line {line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut state = open(&refs, 10_000);
    let file = |state: &mut State, first: u32| state.editors.show("e1", first, 40);
    assert!(sent(&file(&mut state, 100)).is_empty());
    let near_the_end = sent(&file(&mut state, 1900));
    assert_eq!(
        near_the_end,
        [super::EditorCommand::Read(PageRequest {
            file_id: "main".into(),
            from_line: 1400,
            lines: super::PAGE_LINES,
            attempt: 0
        })]
    );
    // The same view does not ask again while the page is on its way.
    assert!(sent(&file(&mut state, 1901)).is_empty());
}

#[test]
fn a_page_that_arrives_keeps_the_caret_and_a_caret_far_away_waits_for_its_page() {
    let mut state = open_full_page();
    let effects = state.editors.motion("e1", Motion::DocumentEnd, false);
    assert_eq!(
        sent(&effects),
        [super::EditorCommand::Read(PageRequest {
            file_id: "main".into(),
            from_line: 4499,
            lines: super::PAGE_LINES,
            attempt: 0
        })]
    );
    state.apply(read_tail());
    assert_eq!(
        caret(&state),
        TextPosition {
            line: 4999,
            column: 5
        }
    );
    assert!(state.editors.file_of_pane_mut("e1").unwrap().take_reveal());
}

#[test]
fn saving_waits_for_the_edits_and_then_for_the_engine_to_save() {
    let mut state = open(&["x"], 1);
    state.editors.insert("e1", "y");
    assert!(sent(&state.editors.save("e1")).is_empty());
    let after = state.apply(written("main", "e1", 2, 1));
    assert!(
        sent(&after)
            .iter()
            .any(|command| matches!(command, super::EditorCommand::Save(_)))
    );
}

#[test]
fn undo_goes_to_the_engine_and_the_caret_goes_where_the_edit_was() {
    let mut state = open(&["one", "two"], 2);
    let undo = sent(&state.editors.replay("e1", false));
    assert_eq!(
        undo,
        [super::EditorCommand::Replay {
            file_id: "main".into(),
            base: "1".into(),
            redo: false
        }]
    );
    let result = serde_json::from_value(json!({
        "file": doc("main", "e1", 2, 2, true), "applied": true, "changedLines": [1],
        "cursor": {"line": 1, "column": 2}
    }))
    .unwrap();
    let effects = state.apply(Event::Editor(super::EditorEvent::Replayed {
        file_id: "main".into(),
        result: Box::new(result),
    }));
    assert!(matches!(sent(&effects)[0], super::EditorCommand::Read(_)));
    state.apply(read("main", "e1", 2, 0, &["one", "tw"], 2));
    assert_eq!(caret(&state), TextPosition { line: 1, column: 2 });
}

#[test]
fn the_engine_saving_the_file_reads_the_page_again_for_its_highlights() {
    let mut state = open(&["let"], 1);
    let saved = event(
        EventName::FileSaved,
        json!({
            "file": doc("main", "e1", 1, 1, false),
            "by": {"actor": {"role": "human_local", "client": "desktop"}, "name": "you"}
        }),
    );
    let effects = state.apply(saved);
    assert!(
        sent(&effects)
            .iter()
            .any(|command| matches!(command, super::EditorCommand::Read(_)))
    );
    let file = state.editors.file_of_pane("e1").unwrap();
    assert!(file.saved_by.is_some() && !file.doc.dirty);
}

#[test]
fn a_saved_event_of_an_older_revision_does_not_move_the_file_back() {
    let mut state = open(&["let"], 1);
    state.editors.insert("e1", "x");
    state.apply(written("main", "e1", 3, 1));
    let saved = event(
        EventName::FileSaved,
        json!({
            "file": doc("main", "e1", 2, 1, false),
            "by": {"actor": {"role": "human_local", "client": "desktop"}, "name": "you"}
        }),
    );
    state.apply(saved);
    assert_eq!(state.editors.file_of_pane("e1").unwrap().doc.revision, "3");
}

#[test]
fn a_conflict_marks_the_file_with_the_draft_revision() {
    let mut state = open(&["let"], 1);
    state.apply(event(
        EventName::FileConflict,
        json!({"fileId": "main", "baseRevision": "1", "draftRevision": "4", "diskRevision": "3"}),
    ));
    let file = state.editors.file_of_pane("e1").unwrap();
    assert!(file.doc.conflicted);
    assert_eq!(
        (file.doc.revision.as_str(), file.doc.disk_revision.as_str()),
        ("4", "3")
    );
}

#[test]
fn a_pane_that_closes_takes_its_file_with_it() {
    let mut state = open(&["let"], 1);
    state.apply(event(
        EventName::PaneClosed,
        json!({"paneId": "e1", "workspaceId": "w"}),
    ));
    assert!(state.editors.file_of_pane("e1").is_none());
}

#[test]
fn pasting_normalizes_line_breaks_and_refuses_what_the_engine_would() {
    let mut state = open(&["ab"], 1);
    state.editors.motion("e1", Motion::LineEnd, false);
    let effects = state.editor_paste("e1", "x\r\ny\rz");
    assert_eq!(writes(&effects)[0].1[0].text, "x\ny\nz");
    assert_eq!(page_text(&state), ["abx", "y", "z"]);
    assert_eq!(state.editors.file_of_pane("e1").unwrap().lines, 3);
    let too_large = "x".repeat(soyeht_protocol::limits::EDITOR_EDIT_BYTES + 1);
    assert!(state.editor_paste("e1", &too_large).is_empty());
    assert!(state.problem.is_some());
}

#[test]
fn control_backspace_and_delete_take_a_word_and_a_line_break_at_the_edge() {
    let mut state = open(&["let value = 1", "next"], 2);
    state.editors.motion("e1", Motion::LineEnd, false);
    state.editors.delete_word("e1", true);
    assert_eq!(page_text(&state)[0], "let value = ");
    state.editors.delete_word("e1", true);
    assert_eq!(page_text(&state)[0], "let value ");
    state.editors.motion("e1", Motion::LineStart, false);
    state.editors.delete_word("e1", false);
    assert_eq!(page_text(&state)[0], "value ");
    // At the end of a line the key is the plain one: the line break goes.
    state.editors.motion("e1", Motion::LineEnd, false);
    state.editors.delete_word("e1", false);
    assert_eq!(page_text(&state), ["value next"]);
}

#[test]
fn a_file_of_crlf_lines_is_edited_before_its_line_breaks_and_breaks_new_lines_the_same_way() {
    let mut state = open(&["abc\r", "def\r", "ghi"], 3);
    state.editors.motion("e1", Motion::LineEnd, false);
    assert_eq!(caret(&state), TextPosition { line: 0, column: 3 });
    let typed = state.editors.insert("e1", "X");
    assert_eq!(writes(&typed)[0].1[0].range.start.column, 3);
    state.apply(written("main", "e1", 2, 3));
    let broken = state.editors.insert("e1", "\n");
    assert_eq!(writes(&broken)[0].1[0].text, "\r\n");
    assert_eq!(page_text(&state), ["abcX", "", "def", "ghi"]);
    state.apply(written("main", "e1", 3, 4));
    // Backspace at the start of a line takes the whole line break.
    let joined = state.editors.delete("e1", true);
    let edit = &writes(&joined)[0].1[0];
    assert_eq!((edit.range.start.line, edit.range.start.column), (0, 4));
    assert_eq!((edit.range.end.line, edit.range.end.column), (1, 0));
    assert_eq!(page_text(&state)[0], "abcX");
    assert_eq!(
        state.editors.file_of_pane("e1").unwrap().line_ending(),
        "\r\n"
    );
    // A paste of lines takes the file's line break.
    state.apply(written("main", "e1", 4, 3));
    let pasted = state.editor_paste("e1", "p\nq");
    assert_eq!(writes(&pasted)[0].1[0].text, "p\r\nq");
    // The size the engine checks is the one after the line breaks are made CRLF.
    state.apply(written("main", "e1", 5, 3));
    let nearly = "\n".repeat(soyeht_protocol::limits::EDITOR_EDIT_BYTES / 2 + 1);
    assert!(state.problem.is_none());
    assert!(state.editor_paste("e1", &nearly).is_empty());
    assert!(state.problem.is_some());
}

#[test]
fn a_caret_that_is_on_its_way_waits_for_its_own_page() {
    let mut state = open_full_page();
    state.editors.motion("e1", Motion::DocumentEnd, false);
    // An older read arrives first: the caret has not moved, and still waits.
    let older: Vec<String> = (0..3).map(|line| format!("o{line}")).collect();
    let refs: Vec<&str> = older.iter().map(String::as_str).collect();
    state.apply(read("main", "e1", 1, 0, &refs, 5000));
    assert_eq!(caret(&state), TextPosition { line: 0, column: 0 });
    state.apply(read_tail());
    assert_eq!(
        caret(&state),
        TextPosition {
            line: 4999,
            column: 5
        }
    );
}

#[test]
fn a_file_that_got_shorter_than_the_page_is_read_from_its_end_and_a_second_failure_is_the_last() {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(
        vec![pane("e1", "w", "editor")],
        vec![doc("main", "e1", 1, 5000, false)],
    )));
    state.apply(read_tail());
    // The file shrinks to 100 lines; the reload from line 4499 is refused.
    state.apply(event(
        EventName::FileChangedOnDisk,
        json!({"fileId": "main", "diskRevision": "2", "changedLines": []}),
    ));
    let saved = event(
        EventName::FileSaved,
        json!({
            "file": doc("main", "e1", 2, 100, false),
            "by": {"actor": {"role": "human_local", "client": "desktop"}, "name": "you"}
        }),
    );
    state.apply(saved);
    let failed = |state: &mut State, from_line: u32| {
        state.apply(Event::Editor(super::EditorEvent::Failed(
            super::Failure::Read {
                file_id: "main".into(),
                from_line,
                attempt: 0,
                reason: "refused".into(),
            },
        )))
    };
    let retry = sent(&failed(&mut state, 4499));
    let [super::EditorCommand::Read(first)] = &retry[..] else {
        panic!("one read: {retry:?}");
    };
    assert_eq!(first.from_line, 0);
    assert!(sent(&failed(&mut state, 0)).is_empty());
}

#[test]
fn a_reconnect_forgets_what_waited_for_the_edits_that_were_lost() {
    let mut state = open(&["a"], 1);
    state.editors.insert("e1", "x");
    let closing = state.editor_close_tab("e1");
    assert!(sent(&closing).is_empty());
    state.apply(Event::Connected(snapshot(
        vec![pane("e1", "w", "editor")],
        vec![doc("main", "e1", 1, 1, false)],
    )));
    let effects = state.apply(read("main", "e1", 1, 0, &["a"], 1));
    assert!(
        effects
            .iter()
            .all(|effect| !matches!(effect, Effect::Send(Command::ClosePane { .. })))
    );
    let typed = state.editors.insert("e1", "y");
    let acknowledged = state.apply(written("main", "e1", 2, 1));
    assert_eq!(writes(&typed).len(), 1);
    assert!(
        acknowledged
            .iter()
            .all(|effect| !matches!(effect, Effect::Send(Command::ClosePane { .. })))
    );
}

#[test]
fn a_page_cut_short_by_the_engine_is_not_asked_for_again() {
    let texts: Vec<String> = (0..500).map(|line| format!("line {line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut state = open(&refs, 10_000);
    assert!(sent(&state.editors.show("e1", 100, 40)).is_empty());
    assert!(sent(&state.editors.show("e1", 101, 40)).is_empty());
    let beyond = sent(&state.editors.show("e1", 480, 40));
    assert_eq!(beyond.len(), 1);
}

#[test]
fn selecting_and_cutting_copies_the_text_and_deletes_it() {
    let mut state = open(&["hello world"], 1);
    state
        .editors
        .click("e1", TextPosition { line: 0, column: 0 }, false);
    state.editors.motion("e1", Motion::WordRight, true);
    assert_eq!(state.editors.copy("e1").as_deref(), Some("hello "));
    let (text, effects) = state.editors.cut("e1");
    assert_eq!(text.as_deref(), Some("hello "));
    assert_eq!(writes(&effects)[0].1[0].text, "");
    assert_eq!(page_text(&state), ["world"]);
    let all = state.editors.select_all("e1");
    assert!(all.is_empty());
    assert_eq!(state.editors.copy("e1").as_deref(), Some("world"));
}

#[test]
fn a_double_click_selects_a_word_and_a_triple_click_the_line() {
    let mut state = open(&["let value = 1"], 1);
    state
        .editors
        .select_around("e1", &TextPosition { line: 0, column: 6 }, false);
    assert_eq!(state.editors.copy("e1").as_deref(), Some("value"));
    state
        .editors
        .select_around("e1", &TextPosition { line: 0, column: 6 }, true);
    assert_eq!(state.editors.copy("e1").as_deref(), Some("let value = 1"));
}

#[test]
fn a_search_selects_the_match_and_ignores_the_answer_to_an_older_one() {
    let mut state = open(&["one two", "three two"], 2);
    let query = soyeht_protocol::FileFindQuery {
        text: "two".into(),
        case_sensitive: false,
        whole_word: false,
        regex: false,
    };
    let from = TextPosition { line: 0, column: 0 };
    let first =
        state
            .editors
            .file_of_pane_mut("e1")
            .unwrap()
            .find(query.clone(), from.clone(), true, true);
    let super::EditorCommand::Find { seq, .. } = sent(&first)[0].clone() else {
        panic!("no search");
    };
    let answer = |seq: u64, line: u32, column: u32| {
        Event::Editor(super::EditorEvent::Found {
            seq,
            file_id: "main".into(),
            result: serde_json::from_value(json!({
                "revision": "1", "total": 2,
                "current": {"index": 0, "range": {
                    "start": {"line": line, "column": column},
                    "end": {"line": line, "column": column + 3}}},
                "visible": []
            }))
            .unwrap(),
        })
    };
    // An answer to a search that was replaced is ignored.
    state
        .editors
        .file_of_pane_mut("e1")
        .unwrap()
        .find(query, from, true, true);
    state.apply(answer(seq, 1, 6));
    assert_eq!(caret(&state), TextPosition { line: 0, column: 0 });
    state.apply(answer(seq + 1, 0, 4));
    let file = state.editors.file_of_pane("e1").unwrap();
    assert_eq!(file.selected_text().as_deref(), Some("two"));
    assert_eq!(
        file.selection.range().start,
        TextPosition { line: 0, column: 4 }
    );
}

#[test]
fn the_caret_reaches_the_end_of_a_file_past_a_page_the_engine_cut_short() {
    let texts: Vec<String> = (99_499..99_799).map(|line| format!("l{line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(
        vec![pane("e1", "w", "editor")],
        vec![doc("main", "e1", 1, 100_000, false)],
    )));
    state.apply(read("main", "e1", 1, 99_499, &refs, 100_000));
    let from_lines = |effects: &[Effect]| -> Vec<u32> {
        sent(effects)
            .into_iter()
            .filter_map(|command| match command {
                super::EditorCommand::Read(request) => Some(request.from_line),
                _ => None,
            })
            .collect()
    };
    // The page is cut at 300 lines: the read leads the line by a quarter of that, not by 500.
    let effects = state.editors.motion("e1", Motion::DocumentEnd, false);
    assert_eq!(from_lines(&effects), [99_924]);
    // The engine cuts that read at 50 lines, short of the line: it is read again from the line.
    let short: Vec<String> = (99_924..99_974).map(|line| format!("l{line}")).collect();
    let refs: Vec<&str> = short.iter().map(String::as_str).collect();
    let effects = state.apply(read("main", "e1", 1, 99_924, &refs, 100_000));
    assert_eq!(from_lines(&effects), [99_999]);
    state.apply(read("main", "e1", 1, 99_999, &["l99999"], 100_000));
    assert_eq!(
        caret(&state),
        TextPosition {
            line: 99_999,
            column: 6
        }
    );
}
