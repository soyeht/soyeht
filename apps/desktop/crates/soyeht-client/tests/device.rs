//! A device pairs with a scripted engine that signs what the real one signs, over a real WebSocket.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use serde_json::json;
use soyeht_client::{
    Error,
    device::{Cancel, Credential, PairingFailure, Progress, pair},
};
use soyeht_protocol::device_auth;
use support::{read, send, serve};

mod support;

const ENGINE_ID: &str = "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b";
const CODE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const REQUEST: &str = "5d0b1c7e-2f3a-4a8b-9c6d-1e2f3a4b5c6d";

/// The engine of a pairing: `signer` is the key it answers with, `decision` the person's, which comes
/// at once; none means the person never decides, and the engine returns whether the phone's side of
/// the connection ended.
fn engine(
    signer: SigningKey,
    decision: Option<bool>,
) -> (std::net::SocketAddr, std::thread::JoinHandle<bool>) {
    serve(move |mut socket| {
        send(
            &mut socket,
            &json!({"jsonrpc":"2.0","method":"auth.challenge",
                   "params":{"nonce":STANDARD.encode([9_u8; 32]),"engineId":ENGINE_ID}}),
        );
        let begin = read(&mut socket).unwrap();
        assert_eq!(begin["method"], "pair.begin");
        let params = &begin["params"];
        assert_eq!(params["code"], CODE);
        let device_key: [u8; 32] = STANDARD
            .decode(params["devicePublicKey"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let challenge = [3_u8; 32];
        let proof = signer.sign(&device_auth::engine_pair_bytes(
            REQUEST,
            &challenge,
            &device_key,
        ));
        send(
            &mut socket,
            &json!({"jsonrpc":"2.0","id":begin["id"],"result":{
                "requestId":REQUEST,"challenge":STANDARD.encode(challenge),
                "engineProof":STANDARD.encode(proof.to_bytes())}}),
        );
        let prove = read(&mut socket).unwrap();
        assert_eq!(prove["method"], "pair.prove");
        assert!(device_auth::pair_proof_matches(
            CODE,
            &device_key,
            &challenge,
            params["deviceName"].as_str().unwrap(),
            prove["params"]["proof"].as_str().unwrap(),
        ));
        send(
            &mut socket,
            &json!({"jsonrpc":"2.0","id":prove["id"],"result":{"status":"pending"}}),
        );
        let result = match decision {
            Some(true) => {
                json!({"requestId":REQUEST,"approved":true,"deviceToken":"token-1","deviceId":"device-1"})
            }
            Some(false) => json!({"requestId":REQUEST,"approved":false,"reason":"Not now."}),
            None => return read(&mut socket).is_none(),
        };
        send(
            &mut socket,
            &json!({"jsonrpc":"2.0","method":"pair.result","params":result}),
        );
        true
    })
}

fn link(address: std::net::SocketAddr, engine_key: &VerifyingKey) -> String {
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(engine_key.as_bytes());
    format!("soyeht-next://pair?engineId={ENGINE_ID}&address={address}&code={CODE}&engineKey={key}")
}

#[test]
fn a_device_pairs_with_the_engine_whose_key_the_link_names() {
    let engine_key = SigningKey::from_bytes(&[0x11; 32]);
    let (address, server) = engine(engine_key.clone(), Some(true));
    let mut seen = Vec::new();
    let credential = pair(
        &link(address, &engine_key.verifying_key()),
        "Phone",
        &mut |progress| seen.push(progress),
        &Cancel::default(),
    )
    .unwrap();
    server.join().unwrap();
    assert!(matches!(seen.first(), Some(Progress::Fingerprint(print)) if print.len() == 64));
    assert_eq!(seen.last(), Some(&Progress::WaitingForApproval));
    let saved = credential.to_json().unwrap();
    assert_eq!(Credential::from_json(&saved).unwrap(), credential);
    assert!(saved.contains("token-1") && saved.contains("device-1"));
}

#[test]
fn an_engine_that_does_not_hold_the_key_of_the_link_is_not_paired_with() {
    let impostor = SigningKey::from_bytes(&[0x22; 32]);
    let named = SigningKey::from_bytes(&[0x11; 32]);
    let (address, server) = engine(impostor, Some(true));
    let result = pair(
        &link(address, &named.verifying_key()),
        "Phone",
        &mut |_| {},
        &Cancel::default(),
    );
    // The device stopped before it proved the code, so the scripted engine's later reads fail.
    assert!(server.join().is_err());
    assert!(matches!(
        result,
        Err(Error::Pairing(PairingFailure::NotYourMac))
    ));
}

#[test]
fn a_refusal_on_the_computer_is_told_in_the_computers_words() {
    let engine_key = SigningKey::from_bytes(&[0x11; 32]);
    let (address, server) = engine(engine_key.clone(), Some(false));
    let result = pair(
        &link(address, &engine_key.verifying_key()),
        "Phone",
        &mut |_| {},
        &Cancel::default(),
    );
    server.join().unwrap();
    assert!(matches!(
        result,
        Err(Error::Pairing(PairingFailure::Refused(reason))) if reason == "Not now."
    ));
}

#[test]
fn a_pairing_that_is_cancelled_ends_at_once_and_never_returns_the_access_the_computer_then_gives() {
    let engine_key = SigningKey::from_bytes(&[0x11; 32]);
    // The computer would allow access, but only after the phone has gone.
    let (address, server) = engine(engine_key.clone(), None);
    let cancel = std::sync::Arc::new(Cancel::default());
    let stopper = std::sync::Arc::clone(&cancel);
    let link = link(address, &engine_key.verifying_key());
    let started = std::time::Instant::now();
    let result = pair(
        &link,
        "Phone",
        &mut |progress| {
            if progress == Progress::WaitingForApproval {
                // From another thread, as a person's tap is: the pairing is blocked reading.
                let stopper = std::sync::Arc::clone(&stopper);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    stopper.stop();
                });
            }
        },
        &cancel,
    );
    assert!(
        matches!(result, Err(Error::Pairing(PairingFailure::Cancelled))),
        "{result:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "the cancel waited for a frame"
    );
    assert!(server.join().unwrap(), "the connection was not closed");
    // A pairing that is stopped before it starts connects to nothing.
    let stopped = Cancel::default();
    stopped.stop();
    let result = pair(&link, "Phone", &mut |_| {}, &stopped);
    assert!(
        matches!(result, Err(Error::Pairing(PairingFailure::Cancelled))),
        "{result:?}"
    );
}
