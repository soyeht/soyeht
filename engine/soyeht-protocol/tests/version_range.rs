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

/// The cases of `contracts/vectors/protocol-version.json`, which the Swift client passes as well.
#[test]
fn the_negotiation_gives_the_answers_of_the_vector() {
    use soyeht_protocol::handshake::{accepts_in, negotiate_in};
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/vectors/protocol-version.json"
    );
    let vector: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let number = |case: &Value, key: &str| u32::try_from(case[key].as_u64().unwrap()).unwrap();
    for case in vector["negotiate"].as_array().unwrap() {
        let (min, current) = (number(case, "min"), number(case, "current"));
        assert_eq!(
            negotiate_in(number(case, "client"), min, current).map(u64::from),
            case["result"].as_u64(),
            "{}",
            case["name"]
        );
    }
    for case in vector["welcome"].as_array().unwrap() {
        let (min, current) = (number(case, "min"), number(case, "current"));
        assert_eq!(
            accepts_in(number(case, "welcomed"), min, current),
            case["accepted"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
}
