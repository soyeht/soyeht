//! Attacks on pairing and on the signed `hello` of a paired device: proofs that are forged,
//! replayed on another request or another connection, codes that were never issued, keys that
//! sign for anyone, and connections past the limit. Each is refused with `unauthorized` (or
//! `invalid_params` for a malformed value), and no device is added.
use super::terminal_hooks_and_phone::{Phone, pairing_proof};
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use soyeht_protocol::limits;

fn kind(answer: &Value) -> &str {
    answer["error"]["data"]["kind"]
        .as_str()
        .unwrap_or("<no error>")
}

fn key_text(key: &SigningKey) -> String {
    STANDARD.encode(key.verifying_key().to_bytes())
}

/// The proof of a device that knows `code`, over `challenge` and the name `Test phone`.
fn proof_for(code: &str, key: &SigningKey, challenge: &Value) -> String {
    let derived = Sha256::digest([code.as_bytes(), key.verifying_key().as_bytes()].concat());
    pairing_proof(&derived, challenge)
}

/// `host:port` of the rig's TCP listener, read from the link of a pairing window.
pub(super) fn listener_address(rig: &Rig) -> String {
    // Only the signed app opens a window: the test program stands for it for this one call.
    let marker = rig.path.join("test-attest-local-ui");
    fs::write(&marker, "").unwrap();
    let mut app = rig.client();
    let window = app.call("device.pair.start", json!({"idempotencyKey":"listener"}));
    fs::remove_file(&marker).unwrap();
    address_of(&window)
}

fn address_of(window: &Value) -> String {
    window["uri"]
        .as_str()
        .unwrap()
        .split("address=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_owned()
}

struct Window {
    local: Client,
    address: String,
    code: String,
}

/// A rig whose person has opened a pairing window. The test program stands for the signed app.
fn open_window(rig: &Rig) -> Window {
    fs::write(rig.path.join("test-attest-local-ui"), "").unwrap();
    let mut local = rig.client();
    let window = local.call("device.pair.start", json!({"idempotencyKey":"window"}));
    Window {
        local,
        address: address_of(&window),
        code: window["code"].as_str().unwrap().to_owned(),
    }
}

fn begin(phone: &mut Phone, code: &str, key: &SigningKey, idempotency: &str) -> Value {
    phone.request(
        "pair.begin",
        json!({"code":code,"deviceName":"Test phone","devicePublicKey":key_text(key),
            "idempotencyKey":idempotency}),
    )
}

struct Paired {
    address: String,
    key: SigningKey,
    token: String,
    device: String,
    window: Window,
}

fn pair_device(rig: &Rig, seed: u8) -> Paired {
    let mut window = open_window(rig);
    let key = SigningKey::from_bytes(&[seed; 32]);
    let mut phone = Phone::connect(&window.address);
    let started = begin(&mut phone, &window.code, &key, "begin");
    let started = started["result"].clone();
    let proof = proof_for(&window.code, &key, &started);
    let proved = phone.request(
        "pair.prove",
        json!({"requestId":started["requestId"],"proof":proof,"idempotencyKey":"prove"}),
    );
    assert_eq!(proved["result"]["status"], "pending", "{proved}");
    window.local.call(
        "device.pair.decide",
        json!({"requestId":started["requestId"],"approve":true,"idempotencyKey":"approve"}),
    );
    let result = phone.read();
    assert_eq!(result["params"]["approved"], true, "{result}");
    Paired {
        address: window.address.clone(),
        key,
        token: result["params"]["deviceToken"].as_str().unwrap().to_owned(),
        device: result["params"]["deviceId"].as_str().unwrap().to_owned(),
        window,
    }
}

/// The parameters of a device's `hello` with the given proof and its own nonce.
fn device_hello(token: &str, proof: &str, client_nonce: [u8; 32]) -> Value {
    json!({"client":"phone","clientVersion":"1","protocolVersion":1,"deviceToken":token,
        "proof":proof,"clientNonce":STANDARD.encode(client_nonce)})
}

/// A connection of the paired device that has said `hello`.
fn connected_device(paired: &Paired) -> Phone {
    let mut phone = Phone::connect(&paired.address);
    let welcome = phone.hello(&paired.key, &paired.token);
    assert_eq!(welcome["result"]["role"], "human_device", "{welcome}");
    phone
}

fn devices(local: &mut Client) -> usize {
    local.call("device.list", json!({}))["items"]
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn a_forged_or_misdirected_pairing_proof_never_makes_a_request_pending() {
    let rig = Rig::new();
    let mut window = open_window(&rig);
    let key = SigningKey::from_bytes(&[21; 32]);
    let mut phone = Phone::connect(&window.address);
    let started = begin(&mut phone, &window.code, &key, "begin")["result"].clone();
    let id = started["requestId"].clone();
    let forged = [
        // The proof of someone who does not know the code.
        proof_for("a-code-the-attacker-guessed", &key, &started),
        // The right code with another key's derivation.
        proof_for(&window.code, &SigningKey::from_bytes(&[22; 32]), &started),
        STANDARD.encode([0u8; 32]),
        STANDARD.encode(b""),
    ];
    for (index, proof) in forged.iter().enumerate() {
        let answer = phone.request(
            "pair.prove",
            json!({"requestId":id,"proof":proof,"idempotencyKey":format!("forged-{index}")}),
        );
        assert_eq!(kind(&answer), "unauthorized", "{answer}");
    }
    // A proof that is not base64 is malformed; a long one that is base64 proves nothing.
    let not_base64 = phone.request(
        "pair.prove",
        json!({"requestId":id,"proof":"not base64 !!","idempotencyKey":"forged-text"}),
    );
    assert_eq!(kind(&not_base64), "invalid_params", "{not_base64}");
    let long = phone.request(
        "pair.prove",
        json!({"requestId":id,"proof":"A".repeat(10_000),"idempotencyKey":"forged-long"}),
    );
    assert_eq!(kind(&long), "unauthorized", "{long}");
    // A proof made for another request's challenge, replayed on this one.
    let other_key = SigningKey::from_bytes(&[23; 32]);
    let mut second = Phone::connect(&window.address);
    let other = begin(&mut second, &window.code, &other_key, "begin-2")["result"].clone();
    let replayed = proof_for(&window.code, &other_key, &other);
    let answer = phone.request(
        "pair.prove",
        json!({"requestId":id,"proof":replayed,"idempotencyKey":"replayed"}),
    );
    assert_eq!(kind(&answer), "unauthorized", "{answer}");
    // A request id begun on another connection cannot be proved from this one.
    let mut third = Phone::connect(&window.address);
    let answer = third.request(
        "pair.prove",
        json!({"requestId":id,"proof":proof_for(&window.code, &key, &started),"idempotencyKey":"hijack"}),
    );
    assert_eq!(kind(&answer), "unauthorized", "{answer}");
    assert_eq!(
        answer["error"]["message"],
        "Begin pairing on this connection first"
    );
    // Nothing is waiting for consent and no device exists.
    let (_, requests) = window.local.snapshot();
    let pending = requests
        .iter()
        .filter(|part| part["section"] == "requests")
        .flat_map(|part| part["data"].as_array().unwrap().clone())
        .filter(|request| request["kind"] == "device_pair")
        .count();
    assert_eq!(pending, 0);
    assert_eq!(devices(&mut window.local), 0);
}

#[test]
fn a_pairing_code_that_was_never_issued_is_refused_and_does_not_burn_the_real_one() {
    let rig = Rig::new();
    let mut window = open_window(&rig);
    let key = SigningKey::from_bytes(&[31; 32]);
    let mut phone = Phone::connect(&window.address);
    for guess in 0..30u8 {
        let code = STANDARD
            .encode([guess; 32])
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_owned();
        assert_eq!(code.len(), limits::PAIRING_CODE_CHARS);
        let answer = begin(&mut phone, &code, &key, &format!("guess-{guess}"));
        assert_eq!(kind(&answer), "unauthorized", "{answer}");
        assert_eq!(
            answer["error"]["message"],
            "Pairing code expired or invalid"
        );
    }
    for code in ["", "short", &"x".repeat(5000)] {
        let answer = begin(&mut phone, code, &key, "malformed");
        assert!(
            matches!(kind(&answer), "unauthorized" | "invalid_params"),
            "{answer}"
        );
    }
    assert_eq!(devices(&mut window.local), 0);
    // The person's own window still works for the person's own phone.
    let mut real = Phone::connect(&window.address);
    assert!(
        begin(&mut real, &window.code, &key, "real")
            .get("result")
            .is_some()
    );
}

#[test]
fn a_device_key_that_signs_for_anyone_or_is_not_a_key_cannot_begin_pairing() {
    let rig = Rig::new();
    let window = open_window(&rig);
    let mut weak = vec![STANDARD.encode([0u8; 32])];
    let mut identity = [0u8; 32];
    identity[0] = 1;
    weak.push(STANDARD.encode(identity));
    weak.push(STANDARD.encode([0xffu8; 32]));
    weak.push(STANDARD.encode([1u8; 31]));
    weak.push("%%%".to_owned());
    for (index, key) in weak.iter().enumerate() {
        let mut phone = Phone::connect(&window.address);
        let answer = phone.request(
            "pair.begin",
            json!({"code":window.code,"deviceName":"Test phone","devicePublicKey":key,
                "idempotencyKey":format!("weak-{index}")}),
        );
        assert!(
            matches!(kind(&answer), "unauthorized" | "invalid_params"),
            "{key}: {answer}"
        );
    }
}

#[test]
fn a_hello_with_a_forged_replayed_or_unknown_device_signature_is_refused() {
    let rig = Rig::new();
    let paired = pair_device(&rig, 41);
    let nonce_of = |phone: &Phone| phone.nonce();
    // Signed by a key the device never registered.
    let mut phone = Phone::connect(&paired.address);
    let forger = SigningKey::from_bytes(&[42; 32]);
    let answer = phone.hello(&forger, &paired.token);
    assert_eq!(kind(&answer), "unauthorized");
    assert_eq!(answer["error"]["message"], "Invalid connection signature");
    // A token nobody holds.
    let mut phone = Phone::connect(&paired.address);
    let answer = phone.hello(&paired.key, "a-token-the-engine-never-issued");
    assert_eq!(answer["error"]["message"], "Unknown or revoked device");
    // A signature the device really made on one connection, replayed on another: the engine's
    // nonce differs, so the bytes it signed differ.
    let mut first = Phone::connect(&paired.address);
    let nonce = nonce_of(&first);
    let client_nonce = [5u8; 32];
    let signature = STANDARD.encode(
        paired
            .key
            .sign(&soyeht_engine::auth::hello_bytes(
                &nonce,
                &client_nonce,
                first.challenge["engineId"].as_str().unwrap(),
                &paired.token,
            ))
            .to_bytes(),
    );
    let welcome = first.request(
        "hello",
        device_hello(&paired.token, &signature, client_nonce),
    );
    assert_eq!(welcome["result"]["role"], "human_device");
    let mut replay = Phone::connect(&paired.address);
    assert_ne!(nonce_of(&replay), nonce, "the engine reused a nonce");
    let answer = replay.request(
        "hello",
        device_hello(&paired.token, &signature, client_nonce),
    );
    assert_eq!(kind(&answer), "unauthorized", "{answer}");
    // A truncated or garbled proof.
    for proof in [
        String::new(),
        "AAAA".to_owned(),
        STANDARD.encode([7u8; 64]),
        "!".repeat(88),
    ] {
        let mut phone = Phone::connect(&paired.address);
        let answer = phone.request(
            "hello",
            json!({"client":"phone","clientVersion":"1","protocolVersion":1,
                "deviceToken":paired.token,"proof":proof,"clientNonce":STANDARD.encode([5u8; 32])}),
        );
        assert!(
            matches!(kind(&answer), "unauthorized" | "invalid_params"),
            "{answer}"
        );
    }
    // The device's own HMAC key material is not a token: a proof without a token is refused.
    let mut phone = Phone::connect(&paired.address);
    let mut mac = Hmac::<Sha256>::new_from_slice(b"anything").unwrap();
    mac.update(b"x");
    let answer = phone.request(
        "hello",
        json!({"client":"phone","clientVersion":"1","protocolVersion":1,
            "proof":STANDARD.encode(mac.finalize().into_bytes())}),
    );
    assert_eq!(kind(&answer), "unauthorized");
}

#[test]
fn a_device_holds_no_more_than_the_declared_number_of_connections() {
    let rig = Rig::new();
    let paired = pair_device(&rig, 51);
    let mut held = Vec::new();
    for _ in 0..limits::DEVICE_CONNECTIONS {
        held.push(connected_device(&paired));
    }
    let mut over = Phone::connect(&paired.address);
    let answer = over.hello(&paired.key, &paired.token);
    assert_eq!(kind(&answer), "unauthorized", "{answer}");
    assert_eq!(
        answer["error"]["message"],
        "Device connection limit reached"
    );
    // The connections it already holds keep working.
    assert!(
        held[0]
            .request("workspace.list", json!({}))
            .get("result")
            .is_some()
    );
}

#[test]
fn a_paired_device_cannot_decide_pairings_or_revoke_another_device() {
    let rig = Rig::new();
    let mut paired = pair_device(&rig, 61);
    let mut phone = connected_device(&paired);
    for (method, params) in [
        ("device.pair.start", json!({"idempotencyKey":"d1"})),
        (
            "device.pair.decide",
            json!({"requestId":"any","approve":true,"idempotencyKey":"d2"}),
        ),
        (
            "device.revoke",
            json!({"deviceId":"another-device","idempotencyKey":"d3"}),
        ),
    ] {
        let answer = phone.request(method, params);
        assert_eq!(kind(&answer), "forbidden_role", "{method}: {answer}");
    }
    assert_eq!(devices(&mut paired.window.local), 1);
    assert!(paired.device.len() > 8);
}

/// Opening a pairing window is the signed app's alone, as deciding is (ADR 0163): a shell, the
/// command line or a script on the local endpoint cannot make an approval prompt appear in the app
/// with a device name of its own. The refusal opens no window, so the code it would have shown
/// works for nobody.
#[test]
fn a_shell_that_is_not_the_signed_app_cannot_open_a_pairing_window() {
    let rig = Rig::new();
    let mut shell = rig.client();
    let refused = shell.request(
        "device.pair.start",
        json!({"idempotencyKey":"shell-window"}),
    );
    assert_eq!(kind(&refused), "forbidden_role", "{refused}");
    assert!(refused.get("result").is_none(), "{refused}");
    let message = refused["error"]["message"].as_str().unwrap();
    assert!(message.contains("Soyeht"), "{message}");
    // A replay of the call is refused the same way: the refusal was not recorded as a result.
    let again = shell.request(
        "device.pair.start",
        json!({"idempotencyKey":"shell-window"}),
    );
    assert_eq!(kind(&again), "forbidden_role", "{again}");
    // The app opens one, as before.
    fs::write(rig.path.join("test-attest-local-ui"), "").unwrap();
    let mut app = rig.client();
    let window = app.call("device.pair.start", json!({"idempotencyKey":"app-window"}));
    assert!(
        window["code"].as_str().is_some_and(|code| !code.is_empty()),
        "{window}"
    );
}

/// Deciding a request and revoking a device are the signed app's alone on every system (ADR 0128):
/// a shell, the command line or a script on the local endpoint is refused, and nothing changes.
#[test]
fn a_shell_that_is_not_the_signed_app_cannot_decide_a_pairing_or_revoke_a_device() {
    let rig = Rig::new();
    let mut paired = pair_device(&rig, 71);
    // A second request waits for the person.
    let window = paired
        .window
        .local
        .call("device.pair.start", json!({"idempotencyKey":"second"}));
    let code = window["code"].as_str().unwrap().to_owned();
    let key = SigningKey::from_bytes(&[72; 32]);
    let mut phone = Phone::connect(&paired.address);
    let started = begin(&mut phone, &code, &key, "begin")["result"].clone();
    let proved = phone.request(
        "pair.prove",
        json!({"requestId":started["requestId"],"proof":proof_for(&code, &key, &started),
            "idempotencyKey":"prove"}),
    );
    assert_eq!(proved["result"]["status"], "pending", "{proved}");
    // The test program stops standing for the app: a client that connects now is a shell.
    fs::remove_file(rig.path.join("test-attest-local-ui")).unwrap();
    let mut shell = rig.client();
    let approve = shell.request(
        "device.pair.decide",
        json!({"requestId":started["requestId"],"approve":true,"idempotencyKey":"shell-approve"}),
    );
    assert_eq!(kind(&approve), "forbidden_role", "{approve}");
    let revoke = shell.request(
        "device.revoke",
        json!({"deviceId":paired.device,"idempotencyKey":"shell-revoke"}),
    );
    assert_eq!(kind(&revoke), "forbidden_role", "{revoke}");
    for refusal in [&approve, &revoke] {
        let message = refusal["error"]["message"].as_str().unwrap();
        assert!(message.contains("Soyeht"), "{message}");
    }
    // Nothing changed: the first device is not revoked and the request is still undecided.
    let listed = paired.window.local.call("device.list", json!({}));
    assert_eq!(listed["items"][0]["revoked"], false, "{listed}");
    // The app still can do both.
    let decided = paired.window.local.call(
        "device.pair.decide",
        json!({"requestId":started["requestId"],"approve":true,"idempotencyKey":"app-approve"}),
    );
    assert_eq!(decided["approved"], true, "{decided}");
    let granted = phone.read();
    assert_eq!(granted["params"]["approved"], true, "{granted}");
    paired.window.local.call(
        "device.revoke",
        json!({"deviceId":paired.device,"idempotencyKey":"app-revoke"}),
    );
    // A revoked device leaves the list; the one the app just approved stays.
    let listed = paired.window.local.call("device.list", json!({}));
    let ids: Vec<_> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|device| device["deviceId"].clone())
        .collect();
    assert_eq!(ids, [granted["params"]["deviceId"].clone()], "{listed}");
}

/// A paired device may forget the computer by revoking itself (ADR 0128): the call succeeds, the
/// device leaves the list, and its connection is closed.
#[test]
fn a_paired_device_can_revoke_itself_and_loses_its_connection() {
    use tokio_tungstenite::tungstenite::Message;
    let rig = Rig::new();
    let mut paired = pair_device(&rig, 81);
    let mut phone = connected_device(&paired);
    assert_eq!(devices(&mut paired.window.local), 1);
    phone
        .socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":9000,"method":"device.revoke","params":{
                "deviceId":paired.device,"idempotencyKey":"forget"}})
            .to_string()
            .into(),
        ))
        .unwrap();
    // The engine closes the connection as it revokes, so the answer may or may not arrive first;
    // if it does, it is a success. Read until the close.
    let mut closed = false;
    for _ in 0..20 {
        match phone.socket.read() {
            Ok(Message::Text(text)) => {
                let message: Value = serde_json::from_str(&text).unwrap();
                if message["id"] == 9000 {
                    assert!(message.get("error").is_none(), "{message}");
                }
            }
            Ok(Message::Close(_)) | Err(_) => {
                closed = true;
                break;
            }
            Ok(_) => {}
        }
    }
    assert!(closed, "a revoked device keeps its connection");
    assert_eq!(devices(&mut paired.window.local), 0);
    let mut again = Phone::connect(&paired.address);
    let welcome = again.hello(&paired.key, &paired.token);
    assert_eq!(kind(&welcome), "unauthorized", "{welcome}");
}
