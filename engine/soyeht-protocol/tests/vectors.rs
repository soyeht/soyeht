#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use soyeht_protocol::*;

fn roundtrip<T: Serialize + DeserializeOwned>(value: Value) -> Value {
    serde_json::to_value(serde_json::from_value::<T>(value).expect("decode typed value"))
        .expect("encode typed value")
}

include!("support/dispatch.rs");

#[test]
fn rejects_invalid_types_and_preserves_forward_compatibility() {
    assert!(serde_json::from_value::<PaneState>(serde_json::json!("future_state")).is_err());
    assert!(
        serde_json::from_value::<PaneCloseParams>(serde_json::json!({"paneId":"pane-1"})).is_err()
    );
    assert!(
        serde_json::from_value::<PaneResizeParams>(
            serde_json::json!({"paneId":"pane-1","idempotencyKey":"key-1","cols":"80","rows":24})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<RpcVersion>(serde_json::json!("1.0")).is_err());
    assert!(
        serde_json::from_value::<AuthChallengeEvent>(
            serde_json::json!({"nonce":23,"engineId":"engine-1"})
        )
        .is_err()
    );
    let value: PaneCloseParams = serde_json::from_value(
        serde_json::json!({"paneId":"pane-1","idempotencyKey":"key-1","future":true}),
    )
    .unwrap();
    assert_eq!(value.pane_id, "pane-1");
}

#[test]
fn all_message_vectors_round_trip_through_generated_types() {
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("../../../contracts/vectors/messages.json")).unwrap();
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    for name in schema["$defs"].as_object().unwrap().keys() {
        assert!(
            vectors.iter().any(|v| v["type"] == *name),
            "missing vector for {name}"
        );
    }
    for vector in vectors {
        if vector["name"].as_str().unwrap().starts_with("request:") {
            let request: Request = serde_json::from_value(vector["value"].clone()).unwrap();
            let reencoded = serde_json::to_value(request).unwrap();
            assert_eq!(reencoded["method"], vector["value"]["method"]);
            assert_eq!(reencoded["params"], vector["value"]["params"]);
        }
        assert_eq!(
            dispatch(vector["type"].as_str().unwrap(), vector["value"].clone()),
            vector["value"],
            "{}",
            vector["name"]
        );
    }
}

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn binary_vectors_match_wire_bytes_in_both_directions() {
    use soyeht_protocol::frame::Frame;
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("../../../contracts/vectors/frames.json")).unwrap();
    for vector in vectors {
        let payload = hex(vector["payloadHex"].as_str().unwrap());
        let expected = match vector["kind"].as_u64().unwrap() {
            1 => Frame::Json(payload),
            2 => Frame::Terminal {
                pane_ref: vector["paneRef"].as_u64().unwrap() as u32,
                generation: vector["generation"].as_u64().unwrap() as u32,
                offset: vector["offset"].as_str().unwrap().parse().unwrap(),
                bytes: payload,
            },
            3 => Frame::Attachment {
                header: hex(vector["headerHex"].as_str().unwrap())
                    .try_into()
                    .unwrap(),
                bytes: payload,
            },
            _ => panic!("unknown frame vector"),
        };
        let wire = hex(vector["hex"].as_str().unwrap());
        assert_eq!(Frame::read_from(&wire[..]).unwrap(), Some(expected.clone()));
        let mut encoded = Vec::new();
        expected.write_to(&mut encoded).unwrap();
        assert_eq!(encoded, wire);
    }
}

#[test]
fn agent_updates_require_known_kinds_and_their_payloads() {
    let common = serde_json::json!({"eventId":"1","at":"now","paneId":"pane","generation":1,"seq":"1","localId":"message"});
    for kind in [
        "unknown",
        "agent_message_chunk",
        "tool_call",
        "terminal_output",
    ] {
        let mut value = common.clone();
        value["kind"] = serde_json::json!(kind);
        assert!(serde_json::from_value::<PaneUpdateEvent>(value).is_err());
    }
    assert!(
        serde_json::from_value::<PermissionOption>(
            serde_json::json!({"optionId":"a","kind":"allow_everything","name":"Allow"})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<ConfigValue>(serde_json::json!([])).is_err());
    assert!(serde_json::from_value::<ConfigValue>(serde_json::json!(true)).is_ok());
}

#[test]
fn closed_enums_reject_legacy_and_unknown_values() {
    let vectors: Vec<Value> = serde_json::from_str(include_str!(
        "../../../contracts/vectors/invalid-messages.json"
    ))
    .unwrap();
    for vector in vectors {
        match vector["type"].as_str() {
            Some("PaneState") => {
                assert!(serde_json::from_value::<PaneState>(vector["value"].clone()).is_err())
            }
            Some("ProbeResult") => {
                assert!(serde_json::from_value::<ProbeResult>(vector["value"].clone()).is_err())
            }
            Some("ModelListSource") => {
                assert!(serde_json::from_value::<ModelListSource>(vector["value"].clone()).is_err())
            }
            _ => (),
        }
    }
}
