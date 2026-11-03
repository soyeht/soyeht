//! A device connects to a scripted engine over a real WebSocket: `Engine::start_device` with the
//! engine's proof good, absent, wrong, and with the wrong role. A refused connection exposes no state:
//! the device never asks for a snapshot, and no `Connected` event ever arrives.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde_json::json;
use soyeht_client::{Engine, Event, device::Credential};
use soyeht_protocol::device_auth;
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use support::{read, send, serve};

mod support;

const ENGINE_ID: &str = "6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b";
const DEVICE_ID: &str = "0c9b6a52-1b2f-4f6e-9d3a-7a1e5f4b8c10";
const TOKEN: &str = "device-token-0123";

/// What the scripted engine answers `hello` with.
#[derive(Clone, Copy)]
enum Welcome {
    Good,
    NoProof,
    WrongProof,
    WrongRole,
}

fn bytes32(text: &str) -> [u8; 32] {
    STANDARD.decode(text).unwrap().try_into().unwrap()
}

/// The engine: greets, answers `hello` as `welcome` says, and returns the methods the device
/// asked for afterwards.
fn engine(
    signer: SigningKey,
    device: VerifyingKey,
    welcome: Welcome,
) -> (SocketAddr, std::thread::JoinHandle<Vec<String>>) {
    serve(move |mut socket| {
        let nonce = [9_u8; 32];
        send(
            &mut socket,
            &json!({"jsonrpc":"2.0","method":"auth.challenge",
                    "params":{"nonce":STANDARD.encode(nonce),"engineId":ENGINE_ID}}),
        );
        let hello = read(&mut socket).unwrap();
        assert_eq!(hello["method"], "hello");
        let params = &hello["params"];
        let client_nonce = bytes32(params["clientNonce"].as_str().unwrap());
        // The device proves itself with the bytes of `device_auth`, signed by its own key.
        let signed = device_auth::hello_bytes(&nonce, &client_nonce, ENGINE_ID, TOKEN);
        let proof = STANDARD.decode(params["proof"].as_str().unwrap()).unwrap();
        device
            .verify_strict(&signed, &Signature::from_slice(&proof).unwrap())
            .unwrap();
        let answer = device_auth::engine_hello_bytes(&nonce, &client_nonce, ENGINE_ID, DEVICE_ID);
        let mut result = json!({"engineVersion":"0","protocolVersion":1,"connectionId":"c1",
            "role":"human_device","capabilities":[],"dbId":"db"});
        match welcome {
            Welcome::Good | Welcome::WrongRole => {
                result["engineProof"] = json!(STANDARD.encode(signer.sign(&answer).to_bytes()));
            }
            Welcome::WrongProof => {
                let other =
                    device_auth::engine_hello_bytes(&[1; 32], &client_nonce, ENGINE_ID, DEVICE_ID);
                result["engineProof"] = json!(STANDARD.encode(signer.sign(&other).to_bytes()));
            }
            Welcome::NoProof => {}
        }
        if matches!(welcome, Welcome::WrongRole) {
            result["role"] = json!("human_local");
        }
        send(
            &mut socket,
            &json!({"jsonrpc":"2.0","id":hello["id"],"result":result}),
        );
        let mut asked = Vec::new();
        while let Some(request) = read(&mut socket) {
            let method = request["method"].as_str().unwrap().to_owned();
            match method.as_str() {
                "state.snapshot" => {
                    send(
                        &mut socket,
                        &json!({"jsonrpc":"2.0","method":"state.snapshot_part","params":{
                        "snapshotId":"s1","index":0,"of":1,"section":"workspaces",
                        "data":[{"workspaceId":"w1","name":"api","path":"/work","layout":{}}]}}),
                    );
                    send(
                        &mut socket,
                        &json!({"jsonrpc":"2.0","id":request["id"],"result":{
                        "snapshotId":"s1","cursor":"7","parts":1}}),
                    );
                }
                "events.subscribe" => send(
                    &mut socket,
                    &json!({"jsonrpc":"2.0","id":request["id"],"result":{"subscriptionId":"sub","through":"7"}}),
                ),
                _ => {}
            }
            asked.push(method);
            if asked.len() == 2 {
                break;
            }
        }
        asked
    })
}

fn credential(address: SocketAddr, engine_key: &VerifyingKey, device: &SigningKey) -> Credential {
    Credential::from_json(
        &json!({"engine_id":ENGINE_ID,"device_id":DEVICE_ID,"address":address.to_string(),
                "token":TOKEN,"key":STANDARD.encode(device.to_bytes()),
                "engine_key":STANDARD.encode(engine_key.as_bytes())})
        .to_string(),
    )
    .unwrap()
}

/// Every event until the first that ends the attempt: `Connected`, or the connection ending.
fn first_outcome(inbox: &async_channel::Receiver<Event>) -> Event {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match inbox.try_recv() {
            Ok(event @ (Event::Connected(_) | Event::Disconnected(_))) => return event,
            Ok(_) => {}
            Err(_) => {
                assert!(
                    Instant::now() < deadline,
                    "the device neither connected nor gave up"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn run(welcome: Welcome) -> (Event, Vec<String>) {
    let (signer, device) = (
        SigningKey::from_bytes(&[0x11; 32]),
        SigningKey::from_bytes(&[7; 32]),
    );
    let (address, server) = engine(signer.clone(), device.verifying_key(), welcome);
    let (engine, inbox) = Engine::start_device(
        credential(address, &signer.verifying_key(), &device),
        "android-dev",
    );
    let outcome = first_outcome(&inbox);
    // A refused connection is closed by the device, which ends the server's reading; a good one ends
    // when the server has seen its two requests.
    let asked = server.join().unwrap();
    engine.stop();
    // Nothing else the device says leaks: every event until the engine stopped is a disconnection or a problem.
    while let Ok(event) = inbox.try_recv() {
        assert!(
            !matches!(event, Event::Connected(_)),
            "a refused engine exposed state"
        );
    }
    (outcome, asked)
}

#[test]
fn an_engine_with_a_good_proof_is_connected_and_the_state_is_asked_for() {
    let (outcome, asked) = run(Welcome::Good);
    let Event::Connected(snapshot) = outcome else {
        panic!("not connected: {outcome:?}");
    };
    assert_eq!(snapshot.workspaces.len(), 1);
    assert_eq!(asked, ["state.snapshot", "events.subscribe"]);
}

fn refused(welcome: Welcome, words: &str) {
    let (outcome, asked) = run(welcome);
    let Event::Disconnected(reason) = outcome else {
        panic!("a refused engine connected: {outcome:?}");
    };
    assert!(reason.contains(words), "{reason}");
    assert!(
        asked.is_empty(),
        "the device asked a refused engine for {asked:?}"
    );
}

#[test]
fn an_engine_that_sends_no_proof_is_too_old_and_gets_no_request() {
    refused(Welcome::NoProof, "Update Soyeht on the computer");
}

#[test]
fn an_engine_with_a_wrong_proof_is_not_your_computer_and_gets_no_request() {
    refused(Welcome::WrongProof, "not your computer");
}

#[test]
fn a_welcome_with_the_wrong_role_is_refused_and_gets_no_request() {
    refused(Welcome::WrongRole, "");
}
