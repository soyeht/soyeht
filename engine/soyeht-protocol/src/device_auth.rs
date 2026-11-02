//! The bytes a paired device and the engine sign and prove to each other (ADR 0013, 0132). They are
//! written here once: the engine checks them, and every client that is a device (the Android app's
//! core) makes them. `contracts/vectors/engine-proof.json` holds the exact bytes, and the engine, this
//! module and the Swift client all pass it.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// What a device signs in `hello`: both nonces, so neither side's freshness rests on the other, the
/// engine it means to reach, and its token.
pub fn hello_bytes(
    nonce: &[u8; 32],
    client_nonce: &[u8; 32],
    engine_id: &str,
    device_token: &str,
) -> Vec<u8> {
    let mut bytes = b"soyeht-hello-v2".to_vec();
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(client_nonce);
    bytes.extend_from_slice(engine_id.as_bytes());
    bytes.extend_from_slice(device_token.as_bytes());
    bytes
}

/// What the engine signs in `welcome` for a device: the two nonces of this connection, itself and
/// the device. A phone that holds the engine's key from pairing checks it, so an impostor, which
/// cannot sign, is refused.
pub fn engine_hello_bytes(
    nonce: &[u8; 32],
    client_nonce: &[u8; 32],
    engine_id: &str,
    device_id: &str,
) -> Vec<u8> {
    let mut bytes = b"soyeht-engine-v1".to_vec();
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(client_nonce);
    bytes.extend_from_slice(engine_id.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(device_id.as_bytes());
    bytes
}

/// What the engine signs in `pair.begin`'s answer: the request, the challenge and the key of the
/// device that began, so the phone pairs only with the engine its QR named.
pub fn engine_pair_bytes(request_id: &str, challenge: &[u8; 32], device_key: &[u8; 32]) -> Vec<u8> {
    let mut bytes = b"soyeht-engine-pair-v1".to_vec();
    bytes.extend_from_slice(challenge);
    bytes.extend_from_slice(device_key);
    bytes.extend_from_slice(request_id.as_bytes());
    bytes
}

/// The MAC that proves a device knows the pairing code without sending it: keyed by the hash of the
/// code and the device's key, over the engine's challenge and the name the person will see.
fn pair_mac(
    code: &str,
    device_key: &[u8; 32],
    challenge: &[u8; 32],
    name: &str,
) -> Option<Hmac<Sha256>> {
    let mut derivation = Sha256::new();
    derivation.update(code.as_bytes());
    derivation.update(device_key);
    let mut mac = Hmac::<Sha256>::new_from_slice(&derivation.finalize()).ok()?;
    mac.update(challenge);
    mac.update(name.as_bytes());
    Some(mac)
}

/// The device's `pair.prove` proof, as base64.
pub fn pair_proof(
    code: &str,
    device_key: &[u8; 32],
    challenge: &[u8; 32],
    name: &str,
) -> Option<String> {
    let mac = pair_mac(code, device_key, challenge, name)?;
    Some(STANDARD.encode(mac.finalize().into_bytes()))
}

/// Whether `proof` (base64) is the proof of a device that knows `code`. The comparison takes the same
/// time wherever the two differ.
pub fn pair_proof_matches(
    code: &str,
    device_key: &[u8; 32],
    challenge: &[u8; 32],
    name: &str,
    proof: &str,
) -> bool {
    let Some(mac) = pair_mac(code, device_key, challenge, name) else {
        return false;
    };
    let Ok(proof) = STANDARD.decode(proof) else {
        return false;
    };
    mac.verify_slice(&proof).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

    const VECTORS: &str = include_str!("../../../contracts/vectors/engine-proof.json");

    fn field(vectors: &serde_json::Value, name: &str) -> String {
        vectors[name].as_str().unwrap_or_default().to_owned()
    }

    fn bytes32(vectors: &serde_json::Value, name: &str) -> [u8; 32] {
        STANDARD
            .decode(field(vectors, name))
            .unwrap()
            .try_into()
            .unwrap()
    }

    fn signature(vectors: &serde_json::Value, name: &str) -> Signature {
        Signature::from_slice(&STANDARD.decode(field(vectors, name)).unwrap()).unwrap()
    }

    #[test]
    fn the_signed_bytes_are_the_ones_the_vectors_hold() {
        use ed25519_dalek::Signer as _;
        let vectors: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        let device = SigningKey::from_bytes(&bytes32(&vectors, "deviceSeed"));
        let seed: Vec<u8> = (0..32)
            .map(|index| {
                u8::from_str_radix(
                    &field(&vectors, "engineSeedHex")[index * 2..index * 2 + 2],
                    16,
                )
                .unwrap()
            })
            .collect();
        let engine = SigningKey::from_bytes(&seed.try_into().unwrap());
        let (nonce, client_nonce) = (bytes32(&vectors, "nonce"), bytes32(&vectors, "clientNonce"));
        let (engine_id, device_id) = (field(&vectors, "engineId"), field(&vectors, "deviceId"));

        let hello = hello_bytes(
            &nonce,
            &client_nonce,
            &engine_id,
            &field(&vectors, "deviceToken"),
        );
        assert_eq!(device.sign(&hello), signature(&vectors, "helloProof"));
        let welcome = engine_hello_bytes(&nonce, &client_nonce, &engine_id, &device_id);
        assert_eq!(
            engine.sign(&welcome),
            signature(&vectors, "engineHelloProof")
        );
        let pair = engine_pair_bytes(
            &field(&vectors, "requestId"),
            &bytes32(&vectors, "challenge"),
            device.verifying_key().as_bytes(),
        );
        assert_eq!(engine.sign(&pair), signature(&vectors, "enginePairProof"));
        let key: VerifyingKey = engine.verifying_key();
        assert!(
            key.verify_strict(&welcome, &signature(&vectors, "engineHelloProof"))
                .is_ok()
        );
    }

    #[test]
    fn a_pairing_proof_is_checked_against_the_code_the_key_the_challenge_and_the_name() {
        let (key, challenge) = ([4; 32], [5; 32]);
        let proof = pair_proof("code", &key, &challenge, "Phone").unwrap();
        assert!(pair_proof_matches(
            "code", &key, &challenge, "Phone", &proof
        ));
        assert!(!pair_proof_matches(
            "other", &key, &challenge, "Phone", &proof
        ));
        assert!(!pair_proof_matches(
            "code", &[6; 32], &challenge, "Phone", &proof
        ));
        assert!(!pair_proof_matches("code", &key, &[6; 32], "Phone", &proof));
        assert!(!pair_proof_matches(
            "code", &key, &challenge, "Tablet", &proof
        ));
        assert!(!pair_proof_matches(
            "code",
            &key,
            &challenge,
            "Phone",
            "not base64!"
        ));
    }
}
