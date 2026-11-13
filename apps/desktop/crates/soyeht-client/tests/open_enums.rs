//! The enums the engine may add values to (`x-openEnums`, ADR 0161): this client is built with the
//! `open-enums` feature of `soyeht-protocol`, so it keeps a value it does not know as `Unknown`
//! instead of refusing the whole message, and never writes that value out.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::{ErrorKind, PaneMode, ToolCall, ToolStatus, error_code};

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
fn a_message_with_an_unknown_value_is_still_read() {
    let tool: ToolCall = serde_json::from_value(
        json!({"toolCallId":"t","title":"Read","status":"paused","locations":[]}),
    )
    .unwrap();
    assert_eq!(tool.status, ToolStatus::Unknown);
}

#[test]
fn the_unknown_value_is_never_written_out() {
    // It is not a value of the contract, so a client that wrote it would send a wrong one.
    assert!(serde_json::to_value(ToolStatus::Unknown).is_err());
    assert!(serde_json::to_value(ErrorKind::Unknown).is_err());
    assert_eq!(
        serde_json::to_value(ToolStatus::Failed).unwrap(),
        json!("failed")
    );
}

#[test]
fn an_enum_that_is_not_open_stays_closed() {
    assert!(serde_json::from_value::<PaneMode>(json!("hologram")).is_err());
}
