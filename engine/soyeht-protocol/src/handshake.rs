//! The opening of every connection, validated the same way by every client: the engine
//! sends `auth.challenge` first, the client answers `hello`, and the answer is a [`Welcome`]
//! that must speak a protocol version this client serves and grant the role the client asked for.
//! The versions are a range (ADR 0161): the engine welcomes a client at the lower of the two
//! versions, and each client accepts any welcome inside its own range.

use crate::{
    AuthChallengeEvent, Error, EventName, HelloParams, Result, Role, Welcome,
    limits::{MIN_PROTOCOL_VERSION, PROTOCOL_VERSION},
};
use serde_json::Value;

/// The engine's first message, decoded; anything else is not an engine of this protocol.
pub fn expect_challenge(message: &Value) -> Result<AuthChallengeEvent> {
    if message["method"].as_str() != Some(EventName::AuthChallenge.as_str()) {
        return Err(Error::EngineChallengeExpected);
    }
    Ok(serde_json::from_value(message["params"].clone())?)
}

/// The `hello` a client sends. A client bound to an agent pane passes its binding.
pub fn hello(client: &str, client_version: &str, binding: Option<&str>) -> HelloParams {
    HelloParams {
        client: client.to_owned(),
        client_version: client_version.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        device_token: None,
        proof: None,
        client_nonce: None,
        binding: binding.map(str::to_owned),
        capabilities: None,
    }
}

/// The version the engine speaks to a client that speaks up to `client`, for an engine serving
/// `min` to `current`: the lower of the two, or `None` when the client is older than `min`.
/// `contracts/vectors/protocol-version.json` holds the cases, which the Swift client passes too.
pub fn negotiate_in(client: u32, min: u32, current: u32) -> Option<u32> {
    (client >= min).then_some(client.min(current))
}

/// [`negotiate_in`] for this build's range.
pub fn negotiate(client: u32) -> Option<u32> {
    negotiate_in(client, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION)
}

/// Whether a client serving `min` to `current` can speak the version an engine welcomed it at.
pub fn accepts_in(welcomed: u32, min: u32, current: u32) -> bool {
    (min..=current).contains(&welcomed)
}

/// The welcome must be at a version this client speaks, else the person has to update one side.
fn expect_version(welcome: &Welcome) -> Result<()> {
    if accepts_in(
        welcome.protocol_version,
        MIN_PROTOCOL_VERSION,
        PROTOCOL_VERSION,
    ) {
        Ok(())
    } else {
        Err(Error::UnsupportedProtocolVersion)
    }
}

/// A bound client must be an agent or its chief; an unbound one is the local human.
pub fn expect_welcome(welcome: &Welcome, bound: bool) -> Result<()> {
    let role_matches = if bound {
        matches!(welcome.role, Role::Agent | Role::Chief)
    } else {
        welcome.role == Role::HumanLocal
    };
    expect_version(welcome)?;
    if role_matches {
        Ok(())
    } else {
        Err(Error::UnexpectedHandshake)
    }
}

/// A paired device is a device, not the computer's own human (ADR 0013).
pub fn expect_device_welcome(welcome: &Welcome) -> Result<()> {
    expect_version(welcome)?;
    if welcome.role == Role::HumanDevice {
        Ok(())
    } else {
        Err(Error::UnexpectedHandshake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn welcome(role: &str, version: u32) -> Welcome {
        serde_json::from_value(json!({
            "engineVersion": "1", "protocolVersion": version, "connectionId": "c",
            "role": role, "capabilities": [], "dbId": "d"
        }))
        .unwrap()
    }

    #[test]
    fn the_challenge_must_be_named_and_well_formed() {
        let challenge = json!({
            "method": "auth.challenge", "params": {"nonce": "AA==", "engineId": "e"}
        });
        assert_eq!(expect_challenge(&challenge).unwrap().engine_id, "e");
        assert!(matches!(
            expect_challenge(&json!({"method": "hello", "params": {}})),
            Err(Error::EngineChallengeExpected)
        ));
        assert!(expect_challenge(&json!({"method": "auth.challenge", "params": {}})).is_err());
    }

    #[test]
    fn hello_carries_the_binding_only_when_bound() {
        assert_eq!(hello("c", "1", None).binding, None);
        let bound = hello("c", "1", Some("pane"));
        assert_eq!(bound.binding.as_deref(), Some("pane"));
        assert_eq!(bound.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn the_welcome_must_match_the_version_and_the_requested_role() {
        assert!(expect_welcome(&welcome("human_local", 1), false).is_ok());
        assert!(expect_welcome(&welcome("agent", 1), true).is_ok());
        assert!(expect_welcome(&welcome("chief", 1), true).is_ok());
        assert!(expect_welcome(&welcome("agent", 1), false).is_err());
        assert!(expect_welcome(&welcome("human_local", 1), true).is_err());
        assert!(expect_welcome(&welcome("human_local", PROTOCOL_VERSION + 1), false).is_err());
    }

    #[test]
    fn a_welcome_outside_the_range_asks_for_an_update() {
        let outside = welcome("human_local", PROTOCOL_VERSION + 1);
        assert!(matches!(
            expect_welcome(&outside, false),
            Err(Error::UnsupportedProtocolVersion)
        ));
        let below = welcome("human_local", MIN_PROTOCOL_VERSION - 1);
        assert!(matches!(
            expect_welcome(&below, false),
            Err(Error::UnsupportedProtocolVersion)
        ));
    }

    #[test]
    fn a_device_is_welcomed_as_a_device_only() {
        assert!(expect_device_welcome(&welcome("human_device", 1)).is_ok());
        assert!(expect_device_welcome(&welcome("human_local", 1)).is_err());
        assert!(expect_device_welcome(&welcome("human_device", PROTOCOL_VERSION + 1)).is_err());
    }
}
