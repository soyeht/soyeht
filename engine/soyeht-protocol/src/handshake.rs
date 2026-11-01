//! The opening of every connection, validated the same way by every client: the engine
//! sends `auth.challenge` first, the client answers `hello`, and the answer is a [`Welcome`]
//! that must speak this protocol version and grant the role the client asked for.

use crate::{
    AuthChallengeEvent, Error, EventName, HelloParams, Result, Role, Welcome,
    limits::PROTOCOL_VERSION,
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

/// A bound client must be an agent or its chief; an unbound one is the local human.
pub fn expect_welcome(welcome: &Welcome, bound: bool) -> Result<()> {
    let role_matches = if bound {
        matches!(welcome.role, Role::Agent | Role::Chief)
    } else {
        welcome.role == Role::HumanLocal
    };
    if welcome.protocol_version == PROTOCOL_VERSION && role_matches {
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
}
