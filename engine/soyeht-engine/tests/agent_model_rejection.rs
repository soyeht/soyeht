//! A model the provider rejects must not pass for a normal reply: the turn
//! ends with the `error` stop reason, the transcript says which model was
//! refused, and `model.list` remembers it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};

mod support;
use support::*;

const REFUSED_NOTICE: &str = "The agent refused model \"model-a\": There's an issue with the selected model (model-a). It may not exist or you may not have access to it. Run /model to pick a different model.";

/// Sends `text` to a fresh agent pane and returns the pane, its client and
/// the stop reason of the turn.
fn turn_stop_reason(rig: &Rig, text: &str) -> (Client, String, Value) {
    let (mut client, pane) = rig.agent();
    let mut observer = rig.subscribed_client();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":text,"idempotencyKey":format!("turn-{pane}")}),
    );
    let ended =
        observer.wait_for_notification("pane.turn_ended", |params| params["paneId"] == pane);
    (client, pane, ended["stopReason"].clone())
}

#[test]
fn a_provider_refusal_written_as_a_reply_ends_the_turn_with_an_error() {
    let rig = Rig::new();
    let (mut client, pane, stop_reason) = turn_stop_reason(&rig, "reject-model-as-reply");
    assert_eq!(stop_reason, "error");

    let items = transcript_items(&mut client, &pane);
    assert!(
        items
            .iter()
            .any(|item| item["role"] == "system" && item["text"] == REFUSED_NOTICE),
        "{items:?}"
    );
    let models = client.call(
        "model.list",
        json!({"harness":"test-agent","paneId":pane,"limit":64}),
    );
    let reasons: Vec<_> = models["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|model| model["observedRefusal"]["reason"].as_str())
        .collect();
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("issue with the selected model")),
        "{models}"
    );
}

#[test]
fn replies_that_are_not_the_refusal_of_the_panes_model_stay_normal_answers() {
    let rig = Rig::new();
    for text in [
        // Quotes the whole sentence inside a longer reply (the fake echoes the prompt).
        "The log says: There's an issue with the selected model (model-a). It may not exist or you may not have access to it. What does that mean?",
        // Mentions only the opening.
        "there is an issue with the selected model, says a log",
        // The provider's sentence, but for a model this pane does not use.
        "reject-other-model-as-reply",
        // The provider's sentence, but not the only reply of the turn.
        "reject-model-after-a-message",
    ] {
        let (mut client, pane, stop_reason) = turn_stop_reason(&rig, text);
        assert_eq!(stop_reason, "end_turn", "{text}");
        let items = transcript_items(&mut client, &pane);
        assert!(
            items
                .iter()
                .all(|item| !item["text"].as_str().unwrap().contains("The agent refused")),
            "{text}: {items:?}"
        );
    }
}
