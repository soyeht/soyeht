//! A device pairs with a scripted engine that signs what the real one signs, over a real WebSocket.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use serde_json::{Value, json};
use soyeht_client::{
    Error,
    device::{Credential, PairingFailure, Progress, pair},
};
use soyeht_protocol::device_auth;
use std::net::TcpListener;
use tungstenite::{Message, WebSocket, accept};

const ENGINE_ID: &str = "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b";
const CODE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const REQUEST: &str = "5d0b1c7e-2f3a-4a8b-9c6d-1e2f3a4b5c6d";

fn read(socket: &mut WebSocket<std::net::TcpStream>) -> Value {
    loop {
        match socket.read().unwrap() {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            _ => continue,
        }
    }
}

fn send(socket: &mut WebSocket<std::net::TcpStream>, message: Value) {
    socket.send(Message::text(message.to_string())).unwrap();
}

/// The engine of a pairing: `signer` is the key it answers with, `approve` the person's decision.
fn engine(signer: SigningKey, approve: bool) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        send(
            &mut socket,
            json!({"jsonrpc":"2.0","method":"auth.challenge",
                   "params":{"nonce":STANDARD.encode([9_u8; 32]),"engineId":ENGINE_ID}}),
        );
        let begin = read(&mut socket);
        assert_eq!(begin["method"], "pair.begin");
        let params = &begin["params"];
        assert_eq!(params["code"], CODE);
        let device_key: [u8; 32] = STANDARD
            .decode(params["devicePublicKey"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let challenge = [3_u8; 32];
        let proof = signer.sign(&device_auth::engine_pair_bytes(REQUEST, &challenge, &device_key));
        send(
            &mut socket,
            json!({"jsonrpc":"2.0","id":begin["id"],"result":{
                "requestId":REQUEST,"challenge":STANDARD.encode(challenge),
                "engineProof":STANDARD.encode(proof.to_bytes())}}),
        );
        let prove = read(&mut socket);
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
            json!({"jsonrpc":"2.0","id":prove["id"],"result":{"status":"pending"}}),
        );
        let result = if approve {
            json!({"requestId":REQUEST,"approved":true,"deviceToken":"token-1","deviceId":"device-1"})
        } else {
            json!({"requestId":REQUEST,"approved":false,"reason":"Not now."})
        };
        send(
            &mut socket,
            json!({"jsonrpc":"2.0","method":"pair.result","params":result}),
        );
    });
    (address, handle)
}

fn link(address: std::net::SocketAddr, engine_key: &VerifyingKey) -> String {
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(engine_key.as_bytes());
    format!(
        "soyeht-next://pair?engineId={ENGINE_ID}&address={address}&code={CODE}&engineKey={key}"
    )
}

#[test]
fn a_device_pairs_with_the_engine_whose_key_the_link_names() {
    let engine_key = SigningKey::from_bytes(&[0x11; 32]);
    let (address, server) = engine(engine_key.clone(), true);
    let mut seen = Vec::new();
    let credential = pair(
        &link(address, &engine_key.verifying_key()),
        "Phone",
        &mut |progress| seen.push(progress),
        &|| false,
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
    let (address, server) = engine(impostor, true);
    let result = pair(
        &link(address, &named.verifying_key()),
        "Phone",
        &mut |_| {},
        &|| false,
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
    let (address, server) = engine(engine_key.clone(), false);
    let result = pair(
        &link(address, &engine_key.verifying_key()),
        "Phone",
        &mut |_| {},
        &|| false,
    );
    server.join().unwrap();
    assert!(matches!(
        result,
        Err(Error::Pairing(PairingFailure::Refused(reason))) if reason == "Not now."
    ));
}
