use super::{PairingFailure, bytes32};
use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use soyeht_platform::log::Failure as _;
use soyeht_protocol::{
    AuthChallengeEvent, ErrorKind, HelloParams, Welcome, device_auth, handshake,
};

/// What a paired device keeps: who the computer is, how to reach it, the token the engine gave and the
/// device's own key. The person's phone is the only place it exists; it is never logged, never sent
/// anywhere but to that engine, and prints as nothing.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credential {
    pub(super) engine_id: String,
    pub(super) device_id: String,
    pub(super) address: String,
    pub(super) token: String,
    /// The device's Ed25519 seed, base64.
    pub(super) key: String,
    /// The engine's Ed25519 public key from the pairing link, base64. Absent in access saved before
    /// ADR 0132, which the person has to replace.
    pub(super) engine_key: Option<String>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Credential")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// What the engine's `welcome` has to prove: its signature of this connection's two nonces and the
/// two identities, by the key the pairing link named.
pub struct Expected {
    engine_key: VerifyingKey,
    signed: Vec<u8>,
}

impl Expected {
    pub fn check(&self, welcome: &Welcome) -> Result<()> {
        let proof = welcome
            .engine_proof
            .as_deref()
            .ok_or(Error::Pairing(PairingFailure::EngineTooOld))?;
        verify(&self.engine_key, &self.signed, proof)
    }
}

/// Whether `proof` is the engine's signature of `signed`; a proof that is not one is the engine not
/// proving itself.
pub(super) fn verify(engine_key: &VerifyingKey, signed: &[u8], proof: &str) -> Result<()> {
    let not_the_engine = || Error::Pairing(PairingFailure::NotYourMac);
    let bytes = STANDARD.decode(proof).map_err(|_| not_the_engine())?;
    let signature = Signature::from_slice(&bytes).map_err(|_| not_the_engine())?;
    // Strict, as the engine checks a device: a signature that only a malleable encoding passes is not one.
    engine_key
        .verify_strict(signed, &signature)
        .map_err(|_| not_the_engine())
}

impl Credential {
    pub(super) fn made(
        link: &super::PairingLink,
        device_id: String,
        token: String,
        key: &SigningKey,
    ) -> Self {
        Self {
            engine_id: link.engine_id.clone(),
            device_id,
            address: link.address.to_string(),
            token,
            key: STANDARD.encode(key.to_bytes()),
            engine_key: Some(STANDARD.encode(link.engine_key)),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    /// Where the engine listens.
    pub(crate) fn socket_address(&self) -> Result<std::net::SocketAddr> {
        self.address
            .parse()
            .map_err(|_| Error::Pairing(PairingFailure::PairAgain))
    }

    /// The `hello` for a connection whose greeting was `challenge`, and what its `welcome` must prove.
    pub fn hello(
        &self,
        client: &str,
        version: &str,
        challenge: &AuthChallengeEvent,
    ) -> Result<(HelloParams, Expected)> {
        let nonce = bytes32(&challenge.nonce).filter(|_| challenge.engine_id == self.engine_id);
        let engine_key = self
            .engine_key
            .as_deref()
            .and_then(bytes32)
            .and_then(|bytes| device_auth::verifying_key(&bytes));
        let (Some(nonce), Some(engine_key)) = (nonce, engine_key) else {
            return Err(Error::Pairing(PairingFailure::PairAgain));
        };
        let key = bytes32(&self.key)
            .map(|seed| SigningKey::from_bytes(&seed))
            .ok_or(Error::Pairing(PairingFailure::PairAgain))?;
        let mut client_nonce = [0_u8; 32];
        getrandom::fill(&mut client_nonce)
            .map_err(|_| Error::Invalid("the system has no random numbers"))?;
        let signed = device_auth::hello_bytes(&nonce, &client_nonce, &self.engine_id, &self.token);
        let mut hello = handshake::hello(client, version, None);
        hello.device_token = Some(self.token.clone());
        hello.proof = Some(STANDARD.encode(key.sign(&signed).to_bytes()));
        hello.client_nonce = Some(STANDARD.encode(client_nonce));
        let expected = Expected {
            engine_key,
            signed: device_auth::engine_hello_bytes(
                &nonce,
                &client_nonce,
                &self.engine_id,
                &self.device_id,
            ),
        };
        Ok((hello, expected))
    }

    /// Whether the engine's refusal of `hello` means this access is no good, so that reconnecting is
    /// pointless until the person pairs again. A server that fails to prove itself is not such a
    /// refusal.
    pub fn refused_by(error: &Error) -> bool {
        Self::refused_cause(&error.failure_kind())
    }

    /// The same rule for the cause an [`Event::Problem`](crate::Event::Problem) carries, which is the
    /// failure's kind: the engine's `unauthorized` or `engine_identity_changed`, or this device's own
    /// "pair again".
    pub fn refused_cause(cause: &str) -> bool {
        let refusing = [ErrorKind::Unauthorized, ErrorKind::EngineIdentityChanged];
        refusing
            .iter()
            .any(|kind| serde_json::to_value(kind).is_ok_and(|text| text == cause))
            || cause == PairingFailure::PairAgain.failure_kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    const VECTORS: &str = include_str!("../../../../../../contracts/vectors/engine-proof.json");

    fn vectors() -> serde_json::Value {
        serde_json::from_str(VECTORS).unwrap()
    }

    fn text(name: &str) -> String {
        vectors()[name].as_str().unwrap().to_owned()
    }

    fn credential() -> Credential {
        Credential {
            engine_id: text("engineId"),
            device_id: text("deviceId"),
            address: "127.0.0.1:9".to_owned(),
            token: text("deviceToken"),
            key: text("deviceSeed"),
            engine_key: Some(text("enginePublicKey")),
        }
    }

    fn challenge() -> AuthChallengeEvent {
        AuthChallengeEvent {
            nonce: text("nonce"),
            engine_id: text("engineId"),
        }
    }

    #[test]
    fn the_hello_is_signed_over_both_nonces_and_the_engine_has_to_answer_in_kind() {
        let (hello, expected) = credential().hello("android", "1", &challenge()).unwrap();
        let nonce = bytes32(&text("nonce")).unwrap();
        let client_nonce = bytes32(hello.client_nonce.as_deref().unwrap()).unwrap();
        // The proof is the device's signature of the bytes of `device_auth`.
        let device = SigningKey::from_bytes(&bytes32(&text("deviceSeed")).unwrap());
        let bytes = device_auth::hello_bytes(
            &nonce,
            &client_nonce,
            &text("engineId"),
            &text("deviceToken"),
        );
        assert_eq!(
            hello.proof,
            Some(STANDARD.encode(device.sign(&bytes).to_bytes()))
        );
        assert_eq!(hello.device_token.as_deref(), Some("device-token-0123"));
        // An engine that signs the two nonces with its key passes; one that signs anything else does not.
        let engine = SigningKey::from_bytes(&[0x11; 32]);
        let proof = |signed: &[u8]| Welcome {
            engine_proof: Some(STANDARD.encode(engine.sign(signed).to_bytes())),
            ..welcome()
        };
        let good = device_auth::engine_hello_bytes(
            &nonce,
            &client_nonce,
            &text("engineId"),
            &text("deviceId"),
        );
        assert!(expected.check(&proof(&good)).is_ok());
        let other =
            device_auth::engine_hello_bytes(&nonce, &[1; 32], &text("engineId"), &text("deviceId"));
        assert!(matches!(
            expected.check(&proof(&other)),
            Err(Error::Pairing(PairingFailure::NotYourMac))
        ));
        assert!(matches!(
            expected.check(&Welcome {
                engine_proof: None,
                ..welcome()
            }),
            Err(Error::Pairing(PairingFailure::EngineTooOld))
        ));
    }

    fn welcome() -> Welcome {
        serde_json::from_value(serde_json::json!({
            "engineVersion": "1", "protocolVersion": 1, "connectionId": "c",
            "role": "human_device", "capabilities": [], "dbId": "d"
        }))
        .unwrap()
    }

    #[test]
    fn access_that_cannot_check_the_engine_asks_to_pair_again() {
        let mut old = credential();
        old.engine_key = None;
        assert!(Credential::refused_by(
            &old.hello("android", "1", &challenge()).err().unwrap()
        ));
        let mut other = challenge();
        other.engine_id = "someone-else".to_owned();
        assert!(Credential::refused_by(
            &credential().hello("android", "1", &other).err().unwrap()
        ));
    }

    #[test]
    fn only_the_engines_refusal_of_the_access_and_the_need_to_pair_again_are_a_refusal() {
        for cause in ["unauthorized", "engine_identity_changed", "pair_again"] {
            assert!(Credential::refused_cause(cause), "{cause}");
        }
        for cause in [
            "not_your_mac",
            "timeout",
            "disconnected",
            "io:ConnectionRefused",
            "",
        ] {
            assert!(!Credential::refused_cause(cause), "{cause}");
        }
    }

    #[test]
    fn a_credential_round_trips_and_prints_nothing_secret() {
        let saved = credential().to_json().unwrap();
        assert_eq!(Credential::from_json(&saved).unwrap(), credential());
        let printed = format!("{:?}", credential());
        assert!(!printed.contains(&text("deviceToken")) && !printed.contains(&text("deviceSeed")));
    }
}
