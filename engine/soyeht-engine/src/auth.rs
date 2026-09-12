use crate::errors::{Error, Result};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, VerifyingKey};
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

pub fn hello_bytes(nonce: &[u8; 32], engine_id: &str, device_token: &str) -> Vec<u8> {
    let mut bytes = b"soyeht-hello-v1".to_vec();
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(engine_id.as_bytes());
    bytes.extend_from_slice(device_token.as_bytes());
    bytes
}

pub fn hello_proof(
    key: &VerifyingKey,
    nonce: &[u8; 32],
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
    key.verify_strict(&hello_bytes(nonce, engine_id, device_token), &signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn hello_signature_is_bound_to_connection_engine_and_token() {
        let private = SigningKey::from_bytes(&[7; 32]);
        let key = private.verifying_key();
        let nonce = [9; 32];
        let proof = STANDARD.encode(
            private
                .sign(&hello_bytes(&nonce, "engine-1", "device-token"))
                .to_bytes(),
        );
        assert!(hello_proof(
            &key,
            &nonce,
            "engine-1",
            "device-token",
            &proof
        ));
        assert!(!hello_proof(
            &key,
            &[10; 32],
            "engine-1",
            "device-token",
            &proof
        ));
        assert!(!hello_proof(
            &key,
            &nonce,
            "engine-2",
            "device-token",
            &proof
        ));
        assert!(!hello_proof(
            &key,
            &nonce,
            "engine-1",
            "other-token",
            &proof
        ));
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
