use crate::errors::{Error, Result};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

pub fn random_bytes() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::RandomUnavailable)?;
    Ok(bytes)
}

pub fn token() -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes()?))
}
pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn same_token(left: &str, right: &str) -> bool {
    token_hash(left)
        .iter()
        .zip(token_hash(right))
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

pub fn public_key(text: &str) -> Result<VerifyingKey> {
    let raw: [u8; 32] = STANDARD
        .decode(text)
        .map_err(|_| Error::InvalidPublicKey)?
        .try_into()
        .map_err(|_| Error::InvalidPublicKeyLength)?;
    let key = VerifyingKey::from_bytes(&raw).map_err(|_| Error::InvalidPublicKey)?;
    if key.is_weak() {
        return Err(Error::WeakPublicKey);
    }
    Ok(key)
}

pub fn fingerprint(key: &VerifyingKey) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn pair_proof(
    code: &str,
    key: &VerifyingKey,
    challenge: &[u8; 32],
    name: &str,
    proof: &str,
) -> bool {
    let mut derivation = Sha256::new();
    derivation.update(code.as_bytes());
    derivation.update(key.as_bytes());
    let derived = derivation.finalize();
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&derived) else {
        return false;
    };
    mac.update(challenge);
    mac.update(name.as_bytes());
    let Ok(proof) = STANDARD.decode(proof) else {
        return false;
    };
    mac.verify_slice(&proof).is_ok()
}

/// What a device signs in `hello`: both nonces, so neither side's freshness rests on the other,
/// the engine it means to reach, and its token (ADR 0132).
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

pub fn hello_proof(
    key: &VerifyingKey,
    nonces: (&[u8; 32], &[u8; 32]),
    engine_id: &str,
    device_token: &str,
    proof: &str,
) -> bool {
    let Ok(raw) = STANDARD.decode(proof) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&raw) else {
        return false;
    };
    let bytes = hello_bytes(nonces.0, nonces.1, engine_id, device_token);
    key.verify_strict(&bytes, &signature).is_ok()
}

/// The engine's signing key from its stored seed (64 hex digits).
pub fn engine_key(seed: &str) -> Result<SigningKey> {
    if seed.len() != 64 || !seed.is_ascii() {
        return Err(Error::InvalidEngineKey);
    }
    let mut raw = [0; 32];
    for (byte, pair) in raw.iter_mut().zip(seed.as_bytes().chunks(2)) {
        let digit = |c: u8| char::from(c).to_digit(16).ok_or(Error::InvalidEngineKey);
        // Both digits are below 16, so the byte cannot overflow.
        *byte = (digit(pair[0])? << 4 | digit(pair[1])?) as u8;
    }
    Ok(SigningKey::from_bytes(&raw))
}

/// A fresh seed for [`engine_key`], as hex.
pub fn new_engine_seed() -> Result<String> {
    Ok(random_bytes()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// The engine's public key as a pairing link carries it: URL-safe base64 without padding.
pub fn engine_key_text(key: &SigningKey) -> String {
    URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes())
}

/// What the engine signs in `welcome` for a device: the two nonces of this connection, itself and
/// the device. A phone that holds the engine's key from pairing checks it, so an impostor, which
/// cannot sign, is refused (ADR 0132).
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

pub fn sign(key: &SigningKey, bytes: &[u8]) -> String {
    STANDARD.encode(key.sign(bytes).to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_signature_is_bound_to_both_nonces_the_engine_and_the_token() {
        let private = SigningKey::from_bytes(&[7; 32]);
        let key = private.verifying_key();
        let (nonce, client) = ([9; 32], [8; 32]);
        let proof = sign(
            &private,
            &hello_bytes(&nonce, &client, "engine-1", "device-token"),
        );
        let check = |nonces: (&[u8; 32], &[u8; 32]), engine: &str, token: &str| {
            hello_proof(&key, nonces, engine, token, &proof)
        };
        assert!(check((&nonce, &client), "engine-1", "device-token"));
        assert!(!check((&[10; 32], &client), "engine-1", "device-token"));
        assert!(!check((&nonce, &[10; 32]), "engine-1", "device-token"));
        assert!(!check((&nonce, &client), "engine-2", "device-token"));
        assert!(!check((&nonce, &client), "engine-1", "other-token"));
    }

    #[test]
    fn the_engine_signs_what_a_phone_with_its_key_checks_and_nothing_else_verifies() {
        let engine = engine_key(&"11".repeat(32)).unwrap();
        let (nonce, client) = ([9; 32], [8; 32]);
        let bytes = engine_hello_bytes(&nonce, &client, "engine-1", "device-1");
        let signature =
            Signature::from_slice(&STANDARD.decode(sign(&engine, &bytes)).unwrap()).unwrap();
        let public = engine.verifying_key();
        assert!(public.verify_strict(&bytes, &signature).is_ok());
        for other in [
            engine_hello_bytes(&[1; 32], &client, "engine-1", "device-1"),
            engine_hello_bytes(&nonce, &[1; 32], "engine-1", "device-1"),
            engine_hello_bytes(&nonce, &client, "engine-2", "device-1"),
            engine_hello_bytes(&nonce, &client, "engine-1", "device-2"),
            engine_pair_bytes("device-1", &nonce, &client),
        ] {
            assert!(public.verify_strict(&other, &signature).is_err());
        }
        assert_eq!(engine_key_text(&engine).len(), 43);
    }

    #[test]
    fn the_signed_bytes_are_those_of_the_shared_vector() {
        let vector: serde_json::Value =
            serde_json::from_str(include_str!("../../../contracts/vectors/engine-proof.json"))
                .unwrap();
        let text = |name: &str| vector[name].as_str().unwrap().to_owned();
        let bytes =
            |name: &str| -> [u8; 32] { STANDARD.decode(text(name)).unwrap().try_into().unwrap() };
        let device = SigningKey::from_bytes(&bytes("deviceSeed"));
        let engine = engine_key(&text("engineSeedHex")).unwrap();
        let (nonce, client, challenge) = (bytes("nonce"), bytes("clientNonce"), bytes("challenge"));
        assert_eq!(engine_key_text(&engine), text("engineKeyText"));
        assert_eq!(
            STANDARD.encode(engine.verifying_key().as_bytes()),
            text("enginePublicKey")
        );
        assert_eq!(
            sign(
                &device,
                &hello_bytes(&nonce, &client, &text("engineId"), &text("deviceToken"))
            ),
            text("helloProof")
        );
        assert_eq!(
            sign(
                &engine,
                &engine_hello_bytes(&nonce, &client, &text("engineId"), &text("deviceId"))
            ),
            text("engineHelloProof")
        );
        assert_eq!(
            sign(
                &engine,
                &engine_pair_bytes(
                    &text("requestId"),
                    &challenge,
                    device.verifying_key().as_bytes()
                )
            ),
            text("enginePairProof")
        );
    }

    #[test]
    fn an_engine_seed_is_sixty_four_hex_digits_and_nothing_else() {
        assert_eq!(new_engine_seed().unwrap().len(), 64);
        assert!(engine_key(&new_engine_seed().unwrap()).is_ok());
        for bad in ["", "zz", &"g".repeat(64), &"1".repeat(63), &"é".repeat(32)] {
            assert!(engine_key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pairing_binds_the_code_key_challenge_and_displayed_name() {
        let key = SigningKey::from_bytes(&[7; 32]).verifying_key();
        let challenge = [9; 32];
        let derived = Sha256::digest([b"test-code".as_slice(), key.as_bytes()].concat());
        let mut mac = Hmac::<Sha256>::new_from_slice(&derived).unwrap();
        mac.update(&challenge);
        mac.update("Telefone".as_bytes());
        let proof = STANDARD.encode(mac.finalize().into_bytes());
        assert!(pair_proof(
            "test-code",
            &key,
            &challenge,
            "Telefone",
            &proof
        ));
        assert!(!pair_proof(
            "wrong-code",
            &key,
            &challenge,
            "Telefone",
            &proof
        ));
        assert!(!pair_proof(
            "test-code",
            &key,
            &challenge,
            "Outro nome",
            &proof
        ));
        assert!(!pair_proof(
            "test-code",
            &key,
            &[10; 32],
            "Telefone",
            &proof
        ));
    }

    #[test]
    fn token_comparison_checks_the_complete_digest() {
        assert!(same_token("binding", "binding"));
        assert!(!same_token("binding", "binding-2"));
        assert!(!same_token("", "binding"));
    }
}
