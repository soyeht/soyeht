//! ADR 0161: the contract names the range of protocol versions the engine serves, not one number
//! that a client must match exactly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_protocol::SCHEMA;

#[test]
fn the_contract_declares_a_range_of_protocol_versions() {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let version = &schema["x-protocolVersion"];
    let (min, current) = (version["min"].as_u64(), version["current"].as_u64());
    let (Some(min), Some(current)) = (min, current) else {
        panic!("`x-protocolVersion` must be {{\"min\": N, \"current\": M}}, it is {version}");
    };
    assert!(
        1 <= min && min <= current,
        "the range {min}..={current} is empty"
    );
}
