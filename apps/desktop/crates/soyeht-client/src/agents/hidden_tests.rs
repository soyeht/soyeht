//! An agent working in a workspace nobody shows: the growing text of its replies is left for the
//! reload when the workspace is shown, and nothing else is.

use super::tests::notification;
use super::*;
use crate::{
    Event,
    snapshot::Snapshot,
    testing::{pane, workspace},
};
use serde_json::json;

fn two_workspaces() -> State {
    let mut state = State::default();
    state.apply(Event::Connected(Snapshot {
        workspaces: vec![workspace("w"), workspace("other")],
        panes: vec![pane("a", "w", "agent"), pane("h", "other", "agent")],
        ..Snapshot::default()
    }));
    state
}

fn update(pane: &str, index: usize, text: &str, complete: bool) -> Event {
    notification(
        EventName::PaneUpdate,
        json!({"eventId": "9", "at": "now", "paneId": pane, "kind": "agent_message_chunk",
               "generation": 1, "seq": index.to_string(), "localId": format!("m{index}"),
               "text": text, "complete": complete}),
    )
}

fn tool(status: &str) -> Event {
    notification(
        EventName::PaneUpdate,
        json!({"eventId": "9", "at": "now", "paneId": "h", "kind": "tool_call", "generation": 1,
               "seq": "2", "localId": "row",
               "tool": {"toolCallId": "t", "title": "ls", "status": status, "locations": []}}),
    )
}

fn item(index: usize) -> soyeht_protocol::TranscriptMessage {
    serde_json::from_value(json!({"localId": format!("m{index}"), "generation": 1,
        "seq": index.to_string(), "role": "assistant", "kind": "message",
        "text": "x", "complete": true}))
    .unwrap()
}

#[test]
fn the_growing_text_of_an_agent_in_a_workspace_nobody_shows_comes_with_the_reload() {
    let mut state = two_workspaces();
    let hidden = update("h", 2, "streaming in the other workspace", false);
    assert!(!state.changes_view(&hidden), "no frame for text not shown");
    assert!(state.apply(hidden).is_empty());
    assert!(
        state.agent("h").unwrap().messages().is_empty(),
        "not decoded"
    );
    let shown = update("a", 2, "streaming here", false);
    assert!(state.changes_view(&shown));
    state.apply(shown);
    assert_eq!(state.agent("a").unwrap().messages().len(), 1);

    state.select_workspace("other");
    assert_eq!(
        state.refill_transcript("h"),
        Some(Command::LoadTranscript {
            pane_id: "h".into()
        }),
        "what it missed is read once when it is shown"
    );
    assert!(state.changes_view(&update("h", 2, "now it is followed", false)));
}

#[test]
fn a_finished_message_or_a_tool_call_of_a_hidden_agent_is_still_applied() {
    let mut state = two_workspaces();
    let finished = update("h", 3, "done", true);
    assert!(state.changes_view(&finished));
    state.apply(finished);
    state.apply(tool("in_progress"));
    state.apply(tool("completed"));
    let held = state.agent("h").unwrap().messages();
    assert_eq!(held.len(), 2);
    assert!(
        held.iter()
            .any(|message| message.local_id == "row" && message.complete),
        "the call that finished while hidden is not left open"
    );
}

#[test]
fn a_long_run_in_a_hidden_workspace_leaves_no_hole_when_it_is_shown() {
    let mut state = two_workspaces();
    state.select_workspace("other");
    for index in 1..=50 {
        state.apply(update("h", index, "x", true));
    }
    state.select_workspace("w");
    for index in 51..=450 {
        state.apply(update("h", index, "x", true));
    }
    state.select_workspace("other");
    if state.refill_transcript("h").is_some() {
        state.apply(Event::Transcript {
            pane_id: "h".into(),
            items: (351..=450).map(item).collect(),
            next_cursor: Some("351".into()),
            newest: true,
        });
    }
    let seqs: Vec<u64> = state
        .agent("h")
        .unwrap()
        .messages()
        .iter()
        .map(|message| message.seq.parse().unwrap())
        .collect();
    let jumps: Vec<(u64, u64)> = seqs
        .windows(2)
        .filter(|pair| pair[1] != pair[0] + 1)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    assert!(jumps.is_empty(), "the transcript skips messages: {jumps:?}");
}
