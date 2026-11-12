//! The enums the engine may add values to (`x-openEnums`, ADR 0161): a client reads a value it does
//! not know as `Unknown`, and an enum that is not listed stays closed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::{ErrorKind, PaneMode, ToolStatus, error_code};

#[test]
fn a_value_a_later_engine_adds_reads_as_unknown() {
    let status: ToolStatus = serde_json::from_value(json!("paused")).unwrap();
    assert_eq!(status, ToolStatus::Unknown);
    let known: ToolStatus = serde_json::from_value(json!("failed")).unwrap();
    assert_eq!(known, ToolStatus::Failed);
    let kind: ErrorKind = serde_json::from_value(json!("a_new_error")).unwrap();
    assert_eq!(kind, ErrorKind::Unknown);
    assert_eq!(error_code(&kind), error_code(&ErrorKind::InternalError));
}

#[test]
fn an_enum_that_is_not_open_stays_closed() {
    assert!(serde_json::from_value::<PaneMode>(json!("hologram")).is_err());
}

#[test]
fn every_open_enum_is_an_enum_the_engine_only_sends() {
    let schema: Value = serde_json::from_str(soyeht_protocol::SCHEMA).unwrap();
    for name in schema["x-openEnums"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        assert!(
            schema["$defs"][name]["enum"].is_array(),
            "{name} is not an enum"
        );
    }
}
