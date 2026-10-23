//! Tabs: the editors of a workspace share one slot, and closing a tab or a pane waits for its
//! edits.

use super::tests::*;
use crate::{Command, Effect, Event, State, testing::pane};
use serde_json::json;
use soyeht_protocol::Pane;

/// A state with the editor panes `e1` and `e2`, which show the files `a` and `b`, and `more` panes.
fn two_editors(more: Vec<Pane>) -> State {
    let mut state = State::default();
    let mut panes = vec![pane("e1", "w", "editor"), pane("e2", "w", "editor")];
    panes.extend(more);
    state.apply(Event::Connected(snapshot(
        panes,
        vec![doc("a", "e1", 1, 1, false), doc("b", "e2", 1, 1, false)],
    )));
    state
}

#[test]
fn closing_a_tab_waits_for_the_edits_and_hands_the_view_to_the_tab_before_it() {
    let mut state = two_editors(Vec::new());
    state.apply(read("a", "e1", 1, 0, &["a"], 1));
    state.apply(read("b", "e2", 1, 0, &["b"], 1));
    state.focus_pane("e1");
    state.focus_pane("e2");
    assert_eq!(
        state.editors.active_pane("w", Some("e2"), state.panes()),
        Some("e2".into())
    );
    state.editors.insert("e2", "x");
    let closing = state.editor_close_tab("e2");
    // The tab goes only after its edit: the engine saves the draft when it closes.
    assert!(
        closing
            .iter()
            .all(|effect| !matches!(effect, Effect::Send(Command::ClosePane { .. })))
    );
    assert!(closing.contains(&Effect::FocusPane("e1".into())));
    let acknowledged = state.apply(written("b", "e2", 2, 1));
    assert!(acknowledged.contains(&Effect::Send(Command::ClosePane {
        pane_id: "e2".into()
    })));
}

#[test]
fn closing_the_pane_of_an_editor_waits_for_its_edits_and_any_other_pane_closes_at_once() {
    let mut state = two_editors(vec![pane("t1", "w", "terminal")]);
    state.apply(read("a", "e1", 1, 0, &["a"], 1));
    let close = |pane_id: &str| {
        Effect::Send(Command::ClosePane {
            pane_id: pane_id.into(),
        })
    };
    assert_eq!(state.close_pane_effects("t1"), [close("t1")]);
    state.editors.insert("e1", "x");
    assert!(!state.close_pane_effects("e1").contains(&close("e1")));
    let acknowledged = state.apply(written("a", "e1", 2, 1));
    assert!(acknowledged.contains(&close("e1")));
    // A tab with nothing unsent goes at once, and a pane the replica does not know is left to
    // the engine to refuse.
    assert!(state.close_pane_effects("e2").contains(&close("e2")));
    assert_eq!(state.close_pane_effects("gone"), [close("gone")]);
}

#[test]
fn the_editor_place_focuses_the_tab_that_shows_and_an_empty_one_has_nothing_to_focus() {
    let mut state = two_editors(vec![pane("t1", "w", "terminal")]);
    state.focus_pane("e2");
    state.focus_pane("t1");
    // The terminal has the keyboard, and the editor goes back to the tab it last showed.
    assert_eq!(state.editor_place("w"), [Effect::FocusPane("e2".into())]);
    assert!(state.editor_place("elsewhere").is_empty());
    let mut bare = State::default();
    bare.apply(Event::Connected(snapshot(
        vec![pane("t1", "w", "terminal")],
        vec![],
    )));
    assert!(bare.editor_place("w").is_empty());
}

#[test]
fn every_editor_pane_of_a_workspace_shares_one_slot_that_shows_the_active_tab() {
    let mut state = two_editors(vec![pane("t1", "w", "terminal")]);
    let shown = |state: &State| -> Vec<String> {
        let tree = state.layout("w", |_| true).unwrap();
        let text = serde_json::to_string(&tree).unwrap();
        ["e1", "e2", "t1"]
            .iter()
            .filter(|id| text.contains(&format!("\"{id}\"")))
            .map(ToString::to_string)
            .collect()
    };
    state.focus_pane("e2");
    assert_eq!(shown(&state), ["e2", "t1"]);
    state.focus_pane("e1");
    assert_eq!(shown(&state), ["e1", "t1"]);
    // Going to the terminal leaves the editor that was last active in its slot.
    state.focus_pane("t1");
    assert_eq!(shown(&state), ["e1", "t1"]);
}

#[test]
fn opening_a_file_adds_its_pane_focuses_it_and_shows_its_page() {
    let mut state = State::default();
    state.apply(Event::Connected(snapshot(Vec::new(), Vec::new())));
    let opened = serde_json::from_value(json!({
        "file": doc("main", "e9", 1, 2, false),
        "pane": serde_json::to_value(pane("e9", "w", "editor")).unwrap(),
        "viewport": {
            "file": doc("main", "e9", 1, 2, false), "source": "buffer", "fromLine": 0, "toLine": 2,
            "lines": [
                {"line": 0, "text": "hello", "highlights": []},
                {"line": 1, "text": "world", "highlights": []}
            ]
        }
    }))
    .unwrap();
    let effects = state.apply(Event::Editor(super::EditorEvent::Opened(Box::new(opened))));
    assert!(effects.contains(&Effect::FocusPane("e9".into())));
    assert!(state.panes().iter().any(|pane| pane.pane_id == "e9"));
    let file = state.editors.file_of_pane("e9").unwrap();
    assert_eq!(file.page.as_ref().unwrap().text(1), "world");
}
