//! A screenplay's scenes and elements, as the engine tells them.

use super::*;

fn element(line: u32, kind: &str) -> Value {
    json!({
        "elementId": format!("e{line}"), "kind": kind, "startLine": line, "endLine": line,
        "text": ""
    })
}

const SCRIPT: [&str; 4] = ["INT. ROOM - DAY", "", "ANA", "Hello."];

/// A screenplay whose elements the engine has told.
fn screenplay() -> State {
    let mut state = open(&SCRIPT);
    state.writing_tick();
    state.apply(structure_of(
        1,
        3,
        json!([{
            "sceneId": "s0", "ordinal": 0, "heading": "INT. ROOM - DAY", "line": 0,
            "linkedBeatIds": []
        }]),
        json!([
            element(0, "scene_heading"),
            element(2, "character"),
            element(3, "dialogue")
        ]),
    ));
    state
}

#[test]
fn the_engine_tells_a_screenplay_its_scenes_and_the_element_of_each_line() {
    let state = screenplay();
    assert_eq!(state.writings.of_pane("p1").unwrap().scenes(), Some(1));
    assert_eq!(
        state.writings.kind_at("p1", 2),
        Some(ScreenplayElementKind::Character)
    );
    assert_eq!(state.writings.kind_at("p1", 1), None);
    let styles = state.writings.line_styles("p1", &SCRIPT.join("\n"));
    assert!(styles[0].bold && styles[3].inset);
    assert_eq!(styles[2].alignment, Alignment::Center);
}

#[test]
fn the_lines_keep_their_element_while_the_person_types_and_the_engine_is_not_asked() {
    let mut state = screenplay();
    // A line is added above the dialogue: the dialogue is still the dialogue.
    let typed = "INT. ROOM - DAY\n\nANA\n(softly)\nHello.";
    state.writing_retype("p1", typed, (30, 30)).unwrap();
    assert_eq!(
        state.writings.kind_at("p1", 4),
        Some(ScreenplayElementKind::Dialogue)
    );
    assert_eq!(state.writings.kind_at("p1", 3), None);
}

#[test]
fn an_answer_for_a_text_that_has_moved_on_does_not_change_the_elements() {
    let mut state = screenplay();
    state
        .writing_retype("p1", "INT. ROOM - DAY\n\nANA\nHello.!", (28, 28))
        .unwrap();
    // The edit has not reached the engine: what it answers is about the text before it.
    state.apply(structure_of(1, 9, json!([]), json!([element(0, "action")])));
    assert_eq!(
        state.writings.kind_at("p1", 0),
        Some(ScreenplayElementKind::SceneHeading)
    );
    assert_eq!(
        state.writings.of_pane("p1").unwrap().scenes(),
        Some(0),
        "the count is still taken"
    );
}

#[test]
fn return_ends_the_line_of_the_element_the_caret_is_in() {
    let state = screenplay();
    let text = SCRIPT.join("\n");
    // At the end of the character: the dialogue comes next.
    let (edit, next) = state.screenplay_advance("p1", &text, 19, false).unwrap();
    assert_eq!(&text[edit.range.clone()], "ANA");
    assert_eq!(edit.replacement, "ANA\n\n");
    assert_eq!(next, ScreenplayElementKind::Dialogue);
    // Tab on a line that holds nothing seeds the element in place; a line the engine has not
    // named is action.
    let (edit, next) = state.screenplay_advance("p1", &text, 16, true).unwrap();
    assert_eq!((edit.range, edit.replacement.as_str()), (16..16, "@"));
    assert_eq!(next, ScreenplayElementKind::Character);
    assert!(state.screenplay_advance("p1", &text, 999, false).is_none());
}
