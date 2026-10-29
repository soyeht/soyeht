//! Tabs: the editors of a workspace share one slot, and closing a tab or a pane waits for its
//! edits.

use super::tests::*;
use crate::{
    Command, Effect, Event, State, layout,
    testing::{pane, pane_json, stored_layout},
};
use serde_json::json;
use soyeht_protocol::{EventName, Pane, WorkspacePaneLayoutAxis};

/// A state with the editor panes `e1` to `e{tabs}`, which show the files `a`, `b`, ..., and
/// `more` panes.
fn editors_beside(tabs: usize, more: Vec<Pane>) -> State {
    let mut state = State::default();
    let mut panes: Vec<Pane> = (1..=tabs)
        .map(|n| pane(&format!("e{n}"), "w", "editor"))
        .collect();
    panes.extend(more);
    let files = (1..=tabs)
        .map(|n| {
            doc(
                &char::from(b'a' + n as u8 - 1).to_string(),
                &format!("e{n}"),
                1,
                1,
                false,
            )
        })
        .collect();
    state.apply(Event::Connected(snapshot(panes, files)));
    state
}

/// A state with the editor panes `e1` and `e2`, which show the files `a` and `b`, and `more` panes.
fn two_editors(more: Vec<Pane>) -> State {
    editors_beside(2, more)
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

/// A write the engine refuses drops the edits and reads the engine's text again, so a pane kept
/// open would show text that was never saved: the close goes on, and the person is told.
#[test]
fn a_refused_write_does_not_keep_the_pane_because_the_text_is_read_again() {
    let mut state = two_editors(Vec::new());
    state.apply(read("a", "e1", 1, 0, &["a"], 1));
    state.editors.insert("e1", "x");
    let page = |state: &State| {
        state
            .editors
            .file_of_pane("e1")
            .unwrap()
            .page
            .as_ref()
            .unwrap()
            .text(0)
            .to_owned()
    };
    assert_eq!(page(&state), "xa");
    let close = Effect::Send(Command::ClosePane {
        pane_id: "e1".into(),
    });
    assert!(!state.close_pane_effects("e1").contains(&close));

    let refused = state.apply(Event::Editor(super::EditorEvent::Failed(
        super::Failure::Edit("a".into()),
    )));
    assert!(refused.contains(&close), "{refused:?}");
    state.apply(read("a", "e1", 1, 0, &["a"], 1));
    assert_eq!(page(&state), "a");
}

#[test]
fn closing_a_workspace_waits_for_the_edits_of_its_documents() {
    let mut state = two_editors(vec![pane("t1", "w", "terminal")]);
    state.apply(read("a", "e1", 1, 0, &["a"], 1));
    let close = |workspace_id: &str| {
        Effect::Send(Command::CloseWorkspace {
            workspace_id: workspace_id.into(),
        })
    };
    // A workspace with nothing unsent goes at once, whichever it is.
    assert_eq!(state.close_workspace_effects("other"), [close("other")]);
    state.editors.insert("e1", "x");
    assert!(state.close_workspace_effects("w").is_empty());
    let acknowledged = state.apply(written("a", "e1", 2, 1));
    assert!(acknowledged.contains(&close("w")), "{acknowledged:?}");
    // It was sent once: the next answer has nothing left to release.
    let later = state.apply(saved("a", "e1", 2, 1));
    assert!(!later.contains(&close("w")), "{later:?}");
}

#[test]
fn a_close_that_waited_for_edits_is_not_made_on_the_next_connection() {
    let mut state = two_editors(Vec::new());
    state.apply(read("a", "e1", 1, 0, &["a"], 1));
    state.editors.insert("e1", "x");
    assert!(state.close_workspace_effects("w").is_empty());
    state.apply(Event::Disconnected("lost".into()));
    let acknowledged = state.apply(written("a", "e1", 2, 1));
    assert!(
        !acknowledged
            .iter()
            .any(|effect| matches!(effect, Effect::Send(Command::CloseWorkspace { .. }))),
        "{acknowledged:?}"
    );
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

/// What the engine asks of a stored layout (`workspace.layout.set`): the editor panes share one
/// slot, so at least one of them stands for all, and every other open pane is a leaf once.
fn engine_accepts(tree: &soyeht_protocol::WorkspacePaneLayoutNode, panes: &[Pane]) -> bool {
    let listed = layout::pane_ids(tree);
    let count = |id: &str| listed.iter().filter(|listed| **listed == id).count();
    let is_editor = |pane: &Pane| pane.mode == soyeht_protocol::PaneMode::Editor;
    panes
        .iter()
        .filter(|pane| !is_editor(pane))
        .all(|pane| count(&pane.pane_id) == 1)
        && panes
            .iter()
            .any(|pane| is_editor(pane) && count(&pane.pane_id) == 1)
}

#[test]
fn a_split_beside_several_editor_tabs_stores_one_slot_for_the_editor_and_every_other_pane() {
    for tabs in [2, 3] {
        for active in 1..=tabs {
            let mut state = editors_beside(
                tabs,
                vec![pane("t1", "w", "terminal"), pane("ag", "w", "agent")],
            );
            state.focus_pane(&format!("e{active}"));
            state.expect_split(1, "t1", WorkspacePaneLayoutAxis::Vertical);
            state.apply(Event::Notification {
                name: EventName::PaneCreated,
                params: json!({"eventId": "9", "at": "now", "pane": pane_json("n", "w", "terminal")}),
            });
            let effects = state.apply(Event::Opened {
                ticket: 1,
                pane_id: "n".into(),
            });
            let stored = stored_layout(&effects).expect("the split stores the layout");
            // The active tab holds the editor's slot; the others are not in the tree.
            let listed = layout::pane_ids(&stored);
            let editors: Vec<_> = listed.iter().filter(|id| id.starts_with('e')).collect();
            assert_eq!(editors, [&format!("e{active}").as_str()], "{tabs} tabs");
            assert_eq!(listed.iter().filter(|id| **id == "n").count(), 1);
            assert!(
                engine_accepts(&stored, state.panes()),
                "{tabs} tabs: {listed:?}"
            );
        }
    }
}

#[test]
fn a_stored_slot_of_an_editor_that_is_not_the_active_tab_shows_the_active_tab() {
    let mut state = editors_beside(
        3,
        vec![pane("t1", "w", "terminal"), pane("ag", "w", "agent")],
    );
    // The arrangement was stored while `e1` was the tab on show.
    let stored = json!({"version": 2, "mode": "custom", "tree": {
    "axis": "horizontal", "children": [
        {"weight": 2.0, "node": {"paneId": "e1", "kind": "editor"}},
        {"weight": 1.0, "node": {"axis": "vertical", "children": [
            {"weight": 1.0, "node": {"paneId": "t1", "kind": "terminal"}},
            {"weight": 1.0, "node": {"paneId": "ag", "kind": "agent"}}
        ]}}
    ]}});
    let mut workspace = crate::testing::workspace_json("w");
    workspace["layout"] = stored;
    state.apply(Event::Notification {
        name: EventName::WorkspaceLayoutUpdated,
        params: json!({"eventId": "9", "at": "now", "workspace": workspace}),
    });
    state.focus_pane("e3");
    let tree = state.layout("w", |_| true).unwrap();
    assert_eq!(layout::pane_ids(&tree), ["e3", "t1", "ag"]);
}
