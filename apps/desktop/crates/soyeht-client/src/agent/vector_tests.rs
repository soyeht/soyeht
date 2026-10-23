//! The cases of `contracts/vectors/agent-transcript.json`, which the Mac client passes as well:
//! what the replica holds after a series of updates and pages, and what the header's ticker says.

use super::*;
use crate::testing::{cases, strings};
use serde_json::Value;

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
            .messages
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
