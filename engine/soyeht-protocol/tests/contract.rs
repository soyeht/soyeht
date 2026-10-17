#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::{SCHEMA, contract};

fn is_unknown(error: &contract::Mismatch) -> bool {
    matches!(error, contract::Mismatch::Unknown(_))
}

#[test]
fn every_event_and_every_method_result_has_a_check() {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    for name in schema["x-events"].as_object().unwrap().keys() {
        let error = contract::event(name, &Value::Null).unwrap_err();
        assert!(!is_unknown(&error), "event {name} has no check: {error}");
    }
    for name in schema["x-methods"].as_object().unwrap().keys() {
        let error = contract::result(name, &Value::Null).unwrap_err();
        assert!(
            !is_unknown(&error),
            "result of {name} has no check: {error}"
        );
    }
    for name in schema["$defs"].as_object().unwrap().keys() {
        if let Err(error) = contract::validate(name, &Value::Null) {
            assert!(!is_unknown(&error), "schema {name} has no check: {error}");
        }
    }
}

#[test]
fn a_name_outside_the_contract_is_unknown() {
    assert!(is_unknown(
        &contract::event("pane.invented", &json!({})).unwrap_err()
    ));
    assert!(is_unknown(
        &contract::result("pane.invented", &json!({})).unwrap_err()
    ));
}

#[test]
fn a_value_is_refused_for_what_the_contract_does_not_say() {
    let ended = json!({
        "eventId": "7", "at": "2026-01-01T00:00:00Z",
        "paneId": "pane-1", "promptId": "prompt-1", "stopReason": "error"
    });
    assert!(contract::event("pane.turn_ended", &ended).is_ok());

    let mut misspelt = ended.clone();
    misspelt["paneID"] = misspelt["paneId"].take();
    let error = contract::event("pane.turn_ended", &misspelt).unwrap_err();
    assert!(error.to_string().contains("pane.turn_ended"), "{error}");

    let mut extra = ended.clone();
    extra["reason"] = json!("why");
    let error = contract::event("pane.turn_ended", &extra).unwrap_err();
    assert!(error.to_string().contains("reason is not in the contract"));

    let mut wrong = ended;
    wrong["stopReason"] = json!("exploded");
    assert!(contract::event("pane.turn_ended", &wrong).is_err());
}

#[test]
fn the_event_and_response_vectors_pass_the_checks() {
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("../../../contracts/vectors/messages.json")).unwrap();
    let (mut checked, mut refused) = (0, Vec::new());
    for vector in vectors {
        let name = vector["name"].as_str().unwrap();
        let outcome = if let Some(event) = name.strip_prefix("event:") {
            contract::event(event, &vector["value"]["params"])
        } else if let Some(method) = name.strip_prefix("response:") {
            contract::result(method, &vector["value"]["result"])
        } else {
            continue;
        };
        checked += 1;
        if let Err(error) = outcome {
            refused.push(format!("{name}: {error}"));
        }
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
    assert!(checked > 100, "only {checked} vectors were checked");
}

#[test]
fn a_number_is_the_same_whatever_its_spelling_and_a_null_option_is_named() {
    let state = |confidence: Value| {
        json!({
            "eventId": "7", "at": "2026-01-01T00:00:00Z", "paneId": "pane-1",
            "state": "idle", "source": "protocol", "phrase": "Ready", "confidence": confidence
        })
    };
    assert!(contract::event("pane.state_changed", &state(json!(1))).is_ok());
    assert!(contract::event("pane.state_changed", &state(json!(0.5))).is_ok());
    let mut nulled = json!({
        "eventId": "7", "at": "2026-01-01T00:00:00Z", "paneId": "pane-1",
        "promptId": "prompt-1", "stopReason": "error", "actor": null
    });
    let error = contract::event("pane.turn_ended", &nulled)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("actor is null for an optional field"),
        "{error}"
    );
    nulled.as_object_mut().unwrap().remove("actor");
    assert!(contract::event("pane.turn_ended", &nulled).is_ok());
}
