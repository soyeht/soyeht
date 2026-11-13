//! The enums the engine may add values to (`x-openEnums`, ADR 0161). The engine is built without the
//! `open-enums` feature, so it has no `Unknown` value: it refuses a value it does not know wherever
//! it reads one (an agent's update, a permission option, a stop reason, a stored row) and cannot
//! publish one.
#![cfg(not(feature = "open-enums"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::{PaneMode, PermissionOption, ToolCall, contract};

#[test]
fn every_open_enum_refuses_a_value_it_does_not_know() {
    let schema: Value = serde_json::from_str(soyeht_protocol::SCHEMA).unwrap();
    let open = schema["x-openEnums"].as_array().unwrap();
    assert!(!open.is_empty());
    for name in open {
        let name = name.as_str().unwrap();
        assert!(
            schema["$defs"][name]["enum"].is_array(),
            "{name} is not an enum"
        );
        assert!(
            contract::roundtrip(name, &json!("a_value_nobody_defined")).is_err(),
            "{name} accepted a value it does not know"
        );
        // The name of the client-side catch-all is not a value of the contract either, so nothing
        // the engine publishes can carry it.
        assert!(
            contract::roundtrip(name, &json!("Unknown")).is_err(),
            "{name}"
        );
        // The value the contract does define is still read.
        let known = schema["$defs"][name]["enum"][0].clone();
        assert_eq!(contract::roundtrip(name, &known).unwrap(), known, "{name}");
    }
}

#[test]
fn the_types_an_agent_fills_refuse_a_novel_kind() {
    assert!(
        serde_json::from_value::<PermissionOption>(
            json!({"optionId":"a","kind":"allow_everything","name":"Allow"})
        )
        .is_err()
    );
    let tool = json!({"toolCallId":"t","title":"Read","kind":"read","status":"paused"});
    assert!(serde_json::from_value::<ToolCall>(tool).is_err());
}

#[test]
fn an_enum_that_is_not_open_stays_closed() {
    assert!(serde_json::from_value::<PaneMode>(json!("hologram")).is_err());
}
