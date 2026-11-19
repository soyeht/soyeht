//! The cases of `contracts/vectors/agent-transcript.json`, which the Mac client passes as well:
//! what the replica holds after a series of updates and pages, and what the header's ticker says.

use super::*;
use crate::testing::{cases, strings};
use serde_json::{Value, json};
use soyeht_protocol::client_policy::{CAPS_HELD_MESSAGES, CAPS_OFFSCREEN_MESSAGES};

fn message(value: &Value) -> TranscriptMessage {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn the_replica_holds_what_the_vector_says_after_each_series_of_updates() {
    for case in cases("agent-transcript", "merge") {
        let name = case["name"].as_str().unwrap();
        let mut view = AgentView::default();
        for step in case["steps"].as_array().unwrap() {
            if let Some(update) = step.get("update") {
                view.merge(message(update)).unwrap();
            } else {
                let page = step["page"].as_array().unwrap();
                view.merge_page(page.iter().map(message).collect()).unwrap();
            }
        }
        let held: Vec<Value> = view
            .messages()
            .iter()
            .map(|held| {
                let mut entry = serde_json::json!({
                    "localId": held.local_id, "text": held.text, "complete": held.complete
                });
                if let Some(tool) = &held.tool {
                    entry["status"] = serde_json::to_value(&tool.status).unwrap();
                }
                if let Some(terminal) = &held.terminal {
                    entry["terminalBytes"] = serde_json::json!(terminal.bytes);
                }
                entry
            })
            .collect();
        assert_eq!(Value::Array(held), case["messages"], "{name}");
    }
}

#[test]
fn the_ticker_says_what_the_vector_says() {
    for case in cases("agent-transcript", "ticker") {
        let name = case["name"].as_str().unwrap();
        let mut view = AgentView::default();
        for update in case["messages"].as_array().unwrap() {
            view.merge(message(update)).unwrap();
        }
        let expected = strings(&case["items"]);
        let said = view.ticker(case["title"].as_str());
        assert_eq!(
            said,
            (!expected.is_empty()).then(|| expected.join(TICKER_SEPARATOR)),
            "{name}"
        );
    }
}

#[test]
fn the_vector_lists_as_many_earlier_tool_calls_as_the_policy_keeps() {
    let capped = cases("agent-transcript", "ticker")
        .into_iter()
        .find(|case| {
            case["name"]
                .as_str()
                .is_some_and(|name| name.contains("no more"))
        })
        .unwrap();
    assert_eq!(
        capped["items"].as_array().unwrap().len(),
        1 + soyeht_protocol::client_policy::CAPS_TICKER_HISTORY
    );
}

/// A message of the vector's compact form: a name, or an object that makes it a streaming reply,
/// a tool call, a usage report or an unfinished command output.
fn window_message(entry: &Value, seq: usize) -> TranscriptMessage {
    let role = if entry["user"] == true {
        "user"
    } else {
        "assistant"
    };
    let id = entry.as_str().or_else(|| entry["id"].as_str()).unwrap();
    let status = entry["tool"].as_str();
    let kind = match (status, &entry["usage"], &entry["terminal"]) {
        (Some(_), ..) => "tool",
        (_, Value::Bool(_), _) => "usage",
        (.., Value::Bool(_)) => "terminal",
        _ => "message",
    };
    let finished = !matches!(status, Some("pending" | "in_progress"))
        && entry["streaming"] != true
        && kind != "terminal";
    let mut value = serde_json::json!({
        "localId": id, "generation": 1, "seq": seq.to_string(), "role": role,
        "kind": kind, "text": "", "complete": finished
    });
    if let Some(status) = status {
        value["tool"] = serde_json::json!({
            "toolCallId": id, "title": id, "status": status, "locations": [],
            "content": "tree", "rawInput": "in", "rawOutput": "out"
        });
    }
    message(&value)
}

#[test]
fn the_replica_lets_go_of_what_the_vector_says_it_does_not_need() {
    let vector = crate::testing::vector("agent-transcript");
    let limit = |name: &str| vector["windowLimits"][name].as_u64().unwrap() as usize;
    let limits = held::Limits {
        held: limit("held"),
        offscreen: limit("offscreen"),
        slack: limit("slack"),
    };
    for case in cases("agent-transcript", "window") {
        let name = case["name"].as_str().unwrap();
        let mut view = AgentView {
            shown: case["shown"] == true,
            viewing: case["viewing"].as_str().map(str::to_owned),
            messages: (1..)
                .zip(case["messages"].as_array().unwrap())
                .map(|(seq, entry)| window_message(entry, seq))
                .collect(),
            ..AgentView::default()
        };
        view.cards.older = case["older"].as_str().map(str::to_owned);
        view.trim(limits, case["merged"] != true);
        let kept: Vec<_> = view
            .messages()
            .iter()
            .map(|m| m.local_id.as_str())
            .collect();
        assert_eq!(kept, strings(&case["kept"]), "{name}");
        assert_eq!(
            view.cards.older.as_deref(),
            case["keptOlder"].as_str(),
            "{name}"
        );
    }
}

#[test]
fn a_pane_that_is_not_shown_holds_only_its_window_however_much_arrives() {
    let mut view = AgentView::default();
    for seq in 1..=3_000 {
        let entry = if seq == 3_000 {
            json!({"id": "last", "tool": "completed"})
        } else {
            json!("m")
        };
        let mut held = window_message(&entry, seq);
        held.local_id = format!("m{seq}");
        view.merge(held).unwrap();
    }
    let page = soyeht_protocol::client_policy::PAGE_SIZE_TRANSCRIPT as usize;
    assert!(view.messages().len() <= CAPS_OFFSCREEN_MESSAGES + page);
    assert_eq!(
        view.cards.older.as_deref(),
        Some(view.messages()[0].seq.as_str()),
        "load earlier fetches the rest"
    );
    let tool = view.messages().last().unwrap().tool.as_ref().unwrap();
    assert!(tool.content.is_none() && tool.raw_input.is_none() && tool.raw_output.is_none());
    view.trim(held::Limits::default(), true);
    assert_eq!(
        view.messages().len(),
        CAPS_OFFSCREEN_MESSAGES,
        "leaving trims"
    );
    view.set_shown(true);
    for seq in 3_001..=6_000 {
        let mut held = window_message(&json!("m"), seq);
        held.local_id = format!("m{seq}");
        view.merge(held).unwrap();
    }
    assert!(view.messages().len() <= CAPS_HELD_MESSAGES + page);
}

#[test]
fn the_cursor_after_a_page_is_the_older_of_the_one_held_and_the_pages() {
    for case in cases("agent-transcript", "cursor") {
        let mut view = AgentView::default();
        view.cards.older = case["held"].as_str().map(str::to_owned);
        if let Some(newest) = case["newestHeld"].as_u64() {
            view.merge(window_message(&json!("m"), newest as usize))
                .unwrap();
        }
        view.merge_transcript(Vec::new(), case["page"].as_str().map(str::to_owned))
            .unwrap();
        assert_eq!(
            view.cards.older.as_deref(),
            case["expect"].as_str(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn a_pane_reads_the_newest_page_when_the_vector_says_it_does() {
    let slack = crate::testing::vector("agent-transcript")["windowLimits"]["slack"]
        .as_u64()
        .unwrap() as usize;
    for case in cases("agent-transcript", "refill") {
        let wants = held::wants_newest_page(
            case["shown"] == true,
            case["stale"] == true,
            case["held"].as_u64().unwrap() as usize,
            case["older"] == true,
            slack,
        );
        assert_eq!(wants, case["expect"] == true, "{}", case["name"]);
    }
}
