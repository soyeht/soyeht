//! Keys pressed while the caret is on its way to a page of a long file.

use super::{
    EditorCommand, EditorEvent, Failure,
    selection::Motion,
    tests::{caret, doc, open, page_text, read, sent, writes, written},
};
use crate::{Command, Effect, Event, State};
use serde_json::json;
use soyeht_protocol::limits::EDITOR_EDIT_BYTES;
use soyeht_protocol::{TextEdit, TextPosition, TextRange};

/// A file of 100,000 lines whose first page, `l0` to `l299`, has been read.
fn open_long_file() -> State {
    let texts: Vec<String> = (0..300).map(|line| format!("l{line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    open(&refs, 100_000)
}

/// The lines `from..to` of the long file as a page the engine sent.
fn long_page(from: u32, to: u32, revision: u64) -> Event {
    let texts: Vec<String> = (from..to).map(|line| format!("l{line}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    read("main", "e1", revision, from, &refs, 100_000)
}

fn type_text(state: &mut State, text: &str) -> Vec<Effect> {
    text.chars()
        .flat_map(|ch| state.editors.insert("e1", &ch.to_string()))
        .collect()
}

fn edit_at(line: u32, column: u32, text: &str) -> TextEdit {
    let at = TextPosition { line, column };
    TextEdit {
        range: TextRange {
            start: at.clone(),
            end: at,
        },
        text: text.into(),
    }
}

/// Delivers the engine's answer to every write in `effects`, one after the other, and gathers
/// the edits that were sent.
fn settle_writes(state: &mut State, mut effects: Vec<Effect>, mut revision: u64) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    while let Some((_, sent_edits)) = writes(&effects).into_iter().next() {
        edits.extend(sent_edits);
        revision += 1;
        effects = state.apply(written("main", "e1", revision, 100_000));
    }
    edits
}

#[test]
fn keys_pressed_while_a_jump_is_on_its_way_act_at_its_target_in_order() {
    let mut state = open_long_file();
    // Ctrl+End, `ENDMARK`, Ctrl+Home and `STARTMARK`, all before any page has arrived.
    let mut effects = state.editors.motion("e1", Motion::DocumentEnd, false);
    effects.extend(type_text(&mut state, "ENDMARK"));
    effects.extend(state.editors.motion("e1", Motion::DocumentStart, false));
    effects.extend(type_text(&mut state, "STARTMARK"));
    assert!(
        writes(&effects).is_empty(),
        "nothing is written at the old caret"
    );
    assert_eq!(
        page_text(&state)[0],
        "l0",
        "the page the jump left is not edited"
    );
    // The end lands: `ENDMARK` is typed there, and Ctrl+Home asks for the start.
    let super::EditorCommand::Read(end) = &sent(&effects)[0] else {
        panic!("Ctrl+End reads the page that holds the end");
    };
    let effects = state.apply(long_page(end.from_line, 100_000, 1));
    assert_eq!(page_text(&state).last().unwrap(), "l99999ENDMARK");
    let mut edits = settle_writes(&mut state, effects, 1);
    assert_eq!(
        caret(&state),
        TextPosition {
            line: 99_999,
            column: 13
        }
    );
    // The start lands: `STARTMARK` is typed there.
    let effects = state.apply(long_page(0, 300, 8));
    edits.extend(settle_writes(&mut state, effects, 8));
    assert_eq!(page_text(&state)[0], "STARTMARKl0");
    assert_eq!(caret(&state), TextPosition { line: 0, column: 9 });
    let typed = |line: u32, at: u32, text: &str| {
        text.chars()
            .enumerate()
            .map(|(offset, ch)| edit_at(line, at + offset as u32, &ch.to_string()))
            .collect::<Vec<_>>()
    };
    let wanted = [typed(99_999, 6, "ENDMARK"), typed(0, 0, "STARTMARK")].concat();
    assert_eq!(edits, wanted);
}

/// The first line of the page the last read in `effects` asks for.
fn read_start(effects: &[Effect]) -> u32 {
    sent(effects)
        .into_iter()
        .filter_map(|command| match command {
            EditorCommand::Read(request) => Some(request.from_line),
            _ => None,
        })
        .next_back()
        .expect("a read")
}

fn fail_read(state: &mut State, from_line: u32) -> Vec<Effect> {
    state.apply(Event::Editor(EditorEvent::Failed(Failure::Read {
        file_id: "main".into(),
        from_line,
    })))
}

/// The engine's answer to an undo, with `fields` over a file of two lines.
fn replayed(fields: serde_json::Value) -> Event {
    let mut result = json!({"file": doc("main", "e1", 2, 2, true), "changedLines": []});
    result
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    Event::Editor(EditorEvent::Replayed {
        file_id: "main".into(),
        result: Box::new(serde_json::from_value(result).unwrap()),
    })
}

fn dropped(state: &State) -> u32 {
    state.editors.file_of_pane("e1").unwrap().dropped_keys
}

fn landing(state: &State) -> bool {
    state.editors.file_of_pane("e1").unwrap().is_landing()
}

#[test]
fn a_jump_that_never_lands_drops_the_keys_that_waited_for_it_and_says_so() {
    let mut state = open_long_file();
    let jump = state.editors.motion("e1", Motion::DocumentEnd, false);
    type_text(&mut state, "xyz");
    assert_eq!(dropped(&state), 0);
    fail_read(&mut state, read_start(&jump));
    assert!(!landing(&state));
    assert_eq!(dropped(&state), 3);
    // The notice lasts until the next key, which is done at once.
    let effects = type_text(&mut state, "y");
    assert_eq!(dropped(&state), 0);
    assert_eq!(writes(&effects).len(), 1);
    assert_eq!(page_text(&state)[0], "yl0");
}

#[test]
fn a_page_that_arrives_first_does_not_strand_the_jump_when_the_one_waited_for_fails() {
    let mut state = open_long_file();
    // The view asks for a page far down, and then the caret is sent to the end.
    let scrolled = state.editors.show("e1", 50_000, 30);
    let scroll_start = read_start(&scrolled);
    let jump = state.editors.motion("e1", Motion::DocumentEnd, false);
    let jump_start = read_start(&jump);
    assert_ne!(scroll_start, jump_start);
    type_text(&mut state, "k");
    // The scroll's page arrives first, and then the page of the jump fails.
    state.apply(long_page(scroll_start, scroll_start + 300, 1));
    assert!(landing(&state), "the jump still waits for its own page");
    fail_read(&mut state, jump_start);
    assert!(!landing(&state), "no page is left to bring the caret");
    assert_eq!(dropped(&state), 1);
    let effects = type_text(&mut state, "y");
    assert_eq!(writes(&effects).len(), 1);
}

#[test]
fn the_keys_that_wait_are_bounded_and_runs_of_the_same_jump_are_one() {
    let mut state = open_long_file();
    let jump = state.editors.motion("e1", Motion::DocumentEnd, false);
    // A run of jumps to one end, and of drags, is the last of them.
    for _ in 0..1500 {
        state.editors.motion("e1", Motion::DocumentEnd, false);
    }
    assert_eq!(dropped(&state), 0);
    // Past the bound the newest keys are refused, and the rest are done once the page lands.
    let mut state = open_long_file();
    let jump_again = state.editors.motion("e1", Motion::DocumentEnd, false);
    assert_eq!(read_start(&jump), read_start(&jump_again));
    type_text(&mut state, &"x".repeat(1500));
    assert_eq!(dropped(&state), 500);
    let effects = state.apply(long_page(read_start(&jump_again), 100_000, 1));
    assert_eq!(writes(&effects).len(), 1);
    assert_eq!(
        page_text(&state).last().unwrap(),
        &format!("l99999{}", "x".repeat(1000))
    );
}

#[test]
fn text_that_waits_is_bounded_in_bytes() {
    let mut state = open_long_file();
    state.editors.motion("e1", Motion::DocumentEnd, false);
    let big = "a".repeat(EDITOR_EDIT_BYTES / 2 + 1);
    assert!(state.editors.paste("e1", &big).is_ok());
    assert_eq!(dropped(&state), 0);
    assert!(state.editors.paste("e1", &big).is_ok());
    assert_eq!(
        dropped(&state),
        1,
        "the second paste does not fit the bound"
    );
}

#[test]
fn closing_waits_for_the_keys_that_wait_for_a_jump() {
    let mut state = open_long_file();
    let jump = state.editors.motion("e1", Motion::DocumentEnd, false);
    type_text(&mut state, "k");
    let closing = state.editor_close_tab("e1");
    let closes = |effects: &[Effect]| {
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Send(Command::ClosePane { .. })))
    };
    assert!(!closes(&closing));
    let landed = state.apply(long_page(read_start(&jump), 100_000, 1));
    assert_eq!(writes(&landed).len(), 1, "the key is written first");
    assert!(!closes(&landed));
    assert!(closes(&state.apply(written("main", "e1", 2, 100_000))));
}

#[test]
fn keys_typed_after_an_undo_wait_for_the_page_it_leaves() {
    let mut state = open(&["one", "two"], 2);
    state.editors.replay("e1", false);
    assert!(landing(&state));
    // Typed before the engine answers, and again after it has, before the page arrives.
    assert!(writes(&type_text(&mut state, "a")).is_empty());
    let answered = state.apply(replayed(
        json!({"applied": true, "cursor": {"line": 1, "column": 2}}),
    ));
    assert!(writes(&answered).is_empty());
    assert!(landing(&state));
    assert!(writes(&type_text(&mut state, "b")).is_empty());
    // The page arrives; the keys are done on it, where the undo put the caret, in order.
    let landed = state.apply(read("main", "e1", 2, 0, &["one", "tw"], 2));
    assert_eq!(page_text(&state), ["one", "twab"]);
    assert_eq!(writes(&landed).len(), 1);
}

#[test]
fn an_undo_that_changed_nothing_lets_the_keys_that_waited_go() {
    let mut state = open(&["one"], 1);
    state.editors.replay("e1", false);
    type_text(&mut state, "a");
    let answered = state.apply(replayed(json!({"applied": false})));
    assert_eq!(writes(&answered).len(), 1);
    assert_eq!(page_text(&state), ["aone"]);
}
