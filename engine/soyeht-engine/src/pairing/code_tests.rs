//! The person's code mode (ADR 0164) through the engine's own dispatch: a window opened by the
//! local human, a device that pairs from another connection, and the decision. The mode is chosen
//! through a hook, so these run on every system.

use super::*;
use crate::{
    dispatch::Outcome,
    hooks::Hooks,
    outbox::{self, Receiver},
};
use ed25519_dalek::SigningKey;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use soyeht_protocol::{ErrorKind, frame::Frame};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Mode(Approval);

impl Hooks for Mode {
    fn pairing_approval(&self) -> Option<Approval> {
        Some(self.0)
    }
}

type Answer = std::result::Result<Value, errors::Error>;

/// A core with the given rule and the traffic of its local connections, which no code may
/// appear in.
struct Rig {
    core: Core,
    directory: PathBuf,
    local_traffic: Vec<String>,
    receivers: Vec<(String, Receiver)>,
}

impl Rig {
    fn new(approval: Approval) -> Self {
        let directory =
            std::env::temp_dir().join(format!("soyeht-pairing-code-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let core = Core::open_with(&directory, Arc::new(Mode(approval))).unwrap();
        Self {
            core,
            directory,
            local_traffic: vec![],
            receivers: vec![],
        }
    }

    /// A connection, local or not, that has said `hello` when it is local.
    fn connect(&mut self, local: bool, signed_ui: bool) -> String {
        let (outbox, receiver) = outbox::channel();
        let connection = self.core.connect(local, signed_ui, outbox).unwrap();
        if local {
            let hello = json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{
                "client":"code-test","clientVersion":"1","protocolVersion":1}});
            self.core.dispatch(&connection, hello).unwrap();
        }
        self.receivers.push((connection.clone(), receiver));
        connection
    }

    fn call(&mut self, connection: &str, method: &str, mut params: Value) -> Answer {
        if params.get("idempotencyKey").is_none() {
            params["idempotencyKey"] = json!(Uuid::new_v4().to_string());
        }
        let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let local = self.core.clients[connection].local;
        let answer = self
            .core
            .dispatch(connection, request)
            .map(|outcome| match outcome {
                Outcome::Reply(value) => value,
                Outcome::Snapshot { parts, result } => {
                    if local {
                        for part in parts {
                            if let Frame::Json(bytes) = part {
                                self.local_traffic.push(String::from_utf8(bytes).unwrap());
                            }
                        }
                    }
                    result
                }
                _ => panic!("{method} did not reply"),
            });
        if local && let Ok(value) = &answer {
            self.local_traffic.push(value.to_string());
        }
        answer
    }

    /// The notifications the connection was sent since the last call, as text.
    async fn notifications(&mut self, connection: &str) -> Vec<Value> {
        let receiver = &mut self
            .receivers
            .iter_mut()
            .find(|(id, _)| id == connection)
            .unwrap()
            .1;
        let mut seen = vec![];
        while let Ok(Some(packet)) =
            tokio::time::timeout(Duration::from_millis(30), receiver.next())
                .await
                .unwrap_or(Ok(None))
        {
            if let Frame::Json(bytes) = packet.frame {
                seen.push(serde_json::from_slice(&bytes).unwrap());
            }
        }
        seen
    }

    /// A window opened by `app`, and the code of its window.
    fn open_window(&mut self, app: &str) -> String {
        let window = self.call(app, "device.pair.start", json!({})).unwrap();
        window["code"].as_str().unwrap().to_owned()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn kind(answer: &Answer) -> ErrorKind {
    errors::kind(answer.as_ref().unwrap_err())
}

fn text(answer: &Answer) -> String {
    answer.as_ref().unwrap_err().to_string()
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// `pair.begin` on `window` from `connection`, as the device with the key of `seed`.
fn begin(rig: &mut Rig, connection: &str, window: &str, name: &str, seed: u8) -> Answer {
    rig.call(
        connection,
        "pair.begin",
        json!({"code":window,"deviceName":name,
            "devicePublicKey":STANDARD.encode(key(seed).verifying_key().to_bytes())}),
    )
}

/// The `pair.result` the connection was sent.
async fn pair_result(rig: &mut Rig, connection: &str) -> Value {
    let sent = rig.notifications(connection).await;
    sent.into_iter()
        .find(|message| message["method"] == "pair.result")
        .unwrap()["params"]
        .clone()
}

/// A device that has begun a pairing with `window` and proved it, as the answer to `pair.prove`.
struct Device {
    connection: String,
    request: String,
    proved: Value,
    prove_params: Value,
}

/// The parameters of `pair.prove` for the answer `begun` to a `pair.begin` on `window`.
fn prove_params(window: &str, seed: u8, name: &str, begun: &Value) -> Value {
    let device_key = key(seed);
    let derived =
        Sha256::digest([window.as_bytes(), device_key.verifying_key().as_bytes()].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&derived).unwrap();
    mac.update(
        &STANDARD
            .decode(begun["challenge"].as_str().unwrap())
            .unwrap(),
    );
    mac.update(name.as_bytes());
    json!({"requestId":begun["requestId"],
        "proof":STANDARD.encode(mac.finalize().into_bytes()),
        "idempotencyKey":Uuid::new_v4().to_string()})
}

fn pair_with(rig: &mut Rig, window: &str, seed: u8, name: &str) -> Device {
    let phone = rig.connect(false, false);
    let begun = begin(rig, &phone, window, name, seed).unwrap();
    let prove_params = prove_params(window, seed, name, &begun);
    let proved = rig
        .call(&phone, "pair.prove", prove_params.clone())
        .unwrap();
    Device {
        connection: phone,
        request: begun["requestId"].as_str().unwrap().to_owned(),
        proved,
        prove_params,
    }
}

fn decide(rig: &mut Rig, app: &str, request: &str, code: Option<&str>) -> Answer {
    let mut params = json!({"requestId":request,"approve":true});
    if let Some(code) = code {
        params["approvalCode"] = json!(code);
    }
    rig.call(app, "device.pair.decide", params)
}

fn devices(rig: &mut Rig, app: &str) -> usize {
    rig.call(app, "device.list", json!({})).unwrap()["items"]
        .as_array()
        .unwrap()
        .len()
}

fn code_of(device: &Device) -> String {
    device.proved["approvalCode"].as_str().unwrap().to_owned()
}

/// The code with a space in the middle, as the phone shows it.
fn grouped(code: &str) -> String {
    format!("{} {}", &code[..3], &code[3..])
}

#[tokio::test]
async fn the_code_the_device_shows_is_six_digits_and_a_replay_gets_the_same_one() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 1, "Test phone");
    let code = code_of(&device);
    assert_eq!(code.len(), 6, "{code}");
    assert!(code.bytes().all(|byte| byte.is_ascii_digit()), "{code}");
    assert_eq!(device.proved["status"], "pending");
    let replay = rig
        .call(
            &device.connection,
            "pair.prove",
            device.prove_params.clone(),
        )
        .unwrap();
    assert_eq!(replay["approvalCode"], code);
    // The request is listed to the connection that opened the window, with no code in it.
    let pending = rig.core.pending_pair_view(&app).unwrap();
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert!(!pending.to_string().contains(&code));
    assert!(
        rig.core
            .capabilities()
            .contains(&"pairing-code".to_string())
    );
}

#[tokio::test]
async fn the_right_code_approves_once_and_a_decision_is_not_remade() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 2, "Test phone");
    let code = code_of(&device);
    rig.notifications(&device.connection).await;
    let key = Uuid::new_v4().to_string();
    let params = json!({"requestId":device.request,"approve":true,
        "approvalCode":grouped(&code),"idempotencyKey":key});
    let approved = rig
        .call(&app, "device.pair.decide", params.clone())
        .unwrap();
    assert_eq!(approved["approved"], true);
    assert_eq!(devices(&mut rig, &app), 1);
    let result = pair_result(&mut rig, &device.connection).await;
    assert_eq!(result["approved"], true);
    assert!(result["deviceToken"].is_string());
    // The same call again is the stored answer, whatever code it carries now.
    let mut again = params.clone();
    again["approvalCode"] = json!("000000");
    assert_eq!(
        rig.call(&app, "device.pair.decide", again).unwrap(),
        approved
    );
    // Another call is too late: the code was single use and the request is decided.
    let other = decide(&mut rig, &app, &device.request, Some(&code));
    assert_eq!(kind(&other), ErrorKind::TransitionRefused);
    assert_eq!(devices(&mut rig, &app), 1);
}

#[tokio::test]
async fn a_missing_or_wrong_code_approves_nothing_and_the_third_wrong_one_denies() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 3, "Test phone");
    let code = code_of(&device);
    let wrong = if code == "000000" { "000001" } else { "000000" };
    rig.notifications(&device.connection).await;
    // Nothing typed: refused, and no try is used.
    for _ in 0..4 {
        let answer = decide(&mut rig, &app, &device.request, None);
        assert_eq!(kind(&answer), ErrorKind::PairingCodeInvalid);
    }
    let first = decide(&mut rig, &app, &device.request, Some(wrong));
    assert_eq!(kind(&first), ErrorKind::PairingCodeInvalid);
    assert!(text(&first).contains("2 tries left"), "{}", text(&first));
    // Not six digits: refused, and no try is used.
    for typo in ["12345", "1234567", "12345x", ""] {
        let typed = decide(&mut rig, &app, &device.request, Some(typo));
        assert_eq!(kind(&typed), ErrorKind::PairingCodeInvalid);
        assert!(text(&typed).contains("six digits"), "{}", text(&typed));
    }
    let second = decide(&mut rig, &app, &device.request, Some(wrong));
    assert!(text(&second).contains("1 tries left"), "{}", text(&second));
    assert_eq!(devices(&mut rig, &app), 0);
    assert!(rig.notifications(&device.connection).await.is_empty());
    let third = decide(&mut rig, &app, &device.request, Some(wrong));
    assert_eq!(kind(&third), ErrorKind::PairingCodeInvalid);
    assert!(text(&third).contains("denied"), "{}", text(&third));
    // The device is told, in the words of this mode.
    let result = pair_result(&mut rig, &device.connection).await;
    assert_eq!(result["approved"], false);
    assert_eq!(result["reason"], "Denied on the computer");
    // The right code is too late, and nothing was added.
    let late = decide(&mut rig, &app, &device.request, Some(&code));
    assert_eq!(kind(&late), ErrorKind::TransitionRefused);
    assert_eq!(devices(&mut rig, &app), 0);
    assert!(
        rig.core
            .pending_pair_view(&app)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_wrong_code_leaves_no_receipt_so_the_same_call_can_be_made_again_with_the_right_one() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 4, "Test phone");
    let key = Uuid::new_v4().to_string();
    let call = |code: &str| {
        json!({"requestId":device.request,"approve":true,"approvalCode":code,
            "idempotencyKey":key})
    };
    let wrong = if code_of(&device) == "000000" {
        "000001"
    } else {
        "000000"
    };
    let refused = rig.call(&app, "device.pair.decide", call(wrong));
    assert_eq!(kind(&refused), ErrorKind::PairingCodeInvalid);
    let receipt =
        StorePort::idempotency_receipt(&rig.core.store, &rig.core.clients[&app].scope, &key)
            .unwrap();
    assert!(receipt.is_none());
    let approved = rig
        .call(&app, "device.pair.decide", call(&code_of(&device)))
        .unwrap();
    assert_eq!(approved["approved"], true);
    // What was stored is the same whatever the code was.
    let receipt =
        StorePort::idempotency_receipt(&rig.core.store, &rig.core.clients[&app].scope, &key)
            .unwrap()
            .unwrap();
    assert_eq!(
        receipt.fingerprint,
        auth::call_fingerprint(
            "device.pair.decide",
            &json!({"requestId":device.request,"approve":true,"idempotencyKey":key})
        )
    );
}

#[tokio::test]
async fn denying_needs_no_code_and_only_from_the_window_s_connection() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let other = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 5, "Test phone");
    let denial = |request: &str| json!({"requestId":request,"approve":false});
    let refused = rig.call(&other, "device.pair.decide", denial(&device.request));
    assert_eq!(kind(&refused), ErrorKind::ForbiddenRole);
    rig.notifications(&device.connection).await;
    let denied = rig
        .call(&app, "device.pair.decide", denial(&device.request))
        .unwrap();
    assert_eq!(denied["approved"], false);
    let result = pair_result(&mut rig, &device.connection).await;
    assert_eq!(result["reason"], "Denied on the computer");
}

#[tokio::test]
async fn another_local_connection_cannot_decide_or_list_a_request_of_a_window_it_did_not_open() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let shell = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 6, "Test phone");
    // Even a shell that somehow has the code is not the window's connection.
    let answer = decide(&mut rig, &shell, &device.request, Some(&code_of(&device)));
    assert_eq!(kind(&answer), ErrorKind::ForbiddenRole);
    assert!(
        rig.core
            .pending_pair_view(&shell)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        rig.core
            .pending_pair_view(&app)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (_, snapshot) = match rig
        .core
        .dispatch(
            &shell,
            json!({"jsonrpc":"2.0","id":2,"method":"state.snapshot","params":{}}),
        )
        .unwrap()
    {
        Outcome::Snapshot { parts, result } => (result, parts),
        _ => panic!("no snapshot"),
    };
    for part in snapshot {
        let Frame::Json(bytes) = part else { continue };
        assert!(!String::from_utf8(bytes).unwrap().contains(&device.request));
    }
    assert_eq!(devices(&mut rig, &app), 0);
    // The window's own connection still can.
    let approved = decide(&mut rig, &app, &device.request, Some(&code_of(&device)));
    assert!(approved.is_ok());
}

#[tokio::test]
async fn a_window_with_no_connection_left_takes_no_new_request() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    rig.core.disconnect(&app).unwrap();
    let phone = rig.connect(false, false);
    let begun = begin(&mut rig, &phone, &window, "Test phone", 7);
    assert_eq!(kind(&begun), ErrorKind::Unauthorized);
}

#[tokio::test]
async fn a_device_is_given_no_code_when_its_window_lost_its_connection_before_it_proved() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let phone = rig.connect(false, false);
    let begun = begin(&mut rig, &phone, &window, "Test phone", 17).unwrap();
    let params = prove_params(&window, 17, "Test phone", &begun);
    rig.core.disconnect(&app).unwrap();
    let proved = rig.call(&phone, "pair.prove", params);
    assert_eq!(kind(&proved), ErrorKind::Unauthorized);
    assert!(
        rig.core
            .pairing
            .codes
            .check(begun["requestId"].as_str().unwrap(), Some("000000"), 0)
            == Verdict::NoCode
    );
}

#[tokio::test]
async fn the_fifth_window_of_a_connection_closes_its_oldest() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let windows: Vec<String> = (0..5).map(|_| rig.open_window(&app)).collect();
    let phone = rig.connect(false, false);
    let oldest = begin(&mut rig, &phone, &windows[0], "Test phone", 8);
    assert_eq!(kind(&oldest), ErrorKind::Unauthorized);
    assert!(begin(&mut rig, &phone, &windows[4], "Test phone", 9).is_ok());
}

#[tokio::test]
async fn a_local_connection_cannot_be_the_new_device() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let begun = begin(&mut rig, &app, &window, "Test phone", 10);
    // A connection that has said hello is refused for that; one that has not, for being local.
    assert_eq!(kind(&begun), ErrorKind::Unauthorized);
    let (outbox, _receiver) = outbox::channel();
    let fresh = rig.core.connect(true, false, outbox).unwrap();
    let begun = begin(&mut rig, &fresh, &window, "Test phone", 10);
    assert_eq!(kind(&begun), ErrorKind::Unauthorized);
    assert!(text(&begun).contains("another device"), "{}", text(&begun));
}

#[tokio::test]
async fn a_device_name_with_a_bidirectional_control_is_refused() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    for name in ["evil\u{202E}fdp", "two\nlines", "\u{2066}hidden"] {
        let phone = rig.connect(false, false);
        let begun = begin(&mut rig, &phone, &window, name, 11);
        assert_eq!(kind(&begun), ErrorKind::InvalidParams, "{name:?}");
    }
    assert!(
        rig.core
            .pending_pair_view(&app)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn the_code_is_in_no_frame_a_local_connection_receives_and_in_nothing_stored() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 12, "Test phone");
    let code = code_of(&device);
    let (_, requests) = (
        rig.call(&app, "state.snapshot", json!({})).unwrap(),
        rig.core.pending_pair_view(&app).unwrap(),
    );
    assert!(!requests.to_string().contains(&code));
    let wrong = if code == "000000" { "000001" } else { "000000" };
    decide(&mut rig, &app, &device.request, Some(wrong)).unwrap_err();
    let key = Uuid::new_v4().to_string();
    rig.call(
        &app,
        "device.pair.decide",
        json!({"requestId":device.request,"approve":true,"approvalCode":code,
            "idempotencyKey":key}),
    )
    .unwrap();
    rig.call(&app, "device.list", json!({})).unwrap();
    let mut local = rig.local_traffic.clone();
    for connection in rig
        .receivers
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>()
    {
        if rig.core.clients[&connection].local {
            local.extend(
                rig.notifications(&connection)
                    .await
                    .iter()
                    .map(Value::to_string),
            );
        }
    }
    assert!(local.len() > 3);
    for traffic in &local {
        assert!(!traffic.contains(&code), "{traffic}");
    }
    // Nor does the store: the requests it keeps, and the receipts of the calls that were made.
    let stored: Vec<Value> =
        StorePort::active_pair_requests(&rig.core.store, rig.core.now_unix()).unwrap();
    assert!(!serde_json::to_string(&stored).unwrap().contains(&code));
    let scope = rig.core.clients[&app].scope.clone();
    let receipt = StorePort::idempotency_receipt(&rig.core.store, &scope, &key)
        .unwrap()
        .unwrap();
    assert!(!receipt.result.to_string().contains(&code));
    assert!(!receipt.fingerprint.contains(&code));
    let stored = rig.core.store.snapshot().unwrap();
    assert!(
        !serde_json::to_string(&stored.sections)
            .unwrap()
            .contains(&code)
    );
}

#[tokio::test]
async fn an_engine_that_restarted_holds_no_code_and_no_window_for_the_old_requests() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let app = rig.connect(true, false);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 13, "Test phone");
    let first = code_of(&device);
    let directory = rig.directory.clone();
    // The engine ends: every code and binding is gone, the store keeps the request.
    drop(std::mem::replace(
        &mut rig.core,
        Core::open_with(&directory, Arc::new(Mode(Approval::PersonsCode))).unwrap(),
    ));
    rig.local_traffic.clear();
    let new_app = rig.connect(true, false);
    // Nobody holds the old window, so nothing can approve what came in on it.
    let answer = decide(&mut rig, &new_app, &device.request, Some(&first));
    assert_eq!(kind(&answer), ErrorKind::ForbiddenRole);
    assert!(
        rig.core
            .pending_pair_view(&new_app)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_signed_app_mode_engine_sends_no_code_offers_no_capability_and_ignores_the_field() {
    let mut rig = Rig::new(Approval::SignedApp);
    let shell = rig.connect(true, false);
    let app = rig.connect(true, true);
    assert_eq!(
        kind(&rig.call(&shell, "device.pair.start", json!({}))),
        ErrorKind::ForbiddenRole
    );
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 14, "Test phone");
    assert!(
        device.proved.get("approvalCode").is_none(),
        "{}",
        device.proved
    );
    assert!(
        !rig.core
            .capabilities()
            .contains(&"pairing-code".to_string())
    );
    // The signed app approves with no code, and a code it sends is ignored.
    assert!(
        rig.core
            .pending_pair_view(&shell)
            .unwrap()
            .as_array()
            .unwrap()
            .len()
            == 1
    );
    let approved = decide(&mut rig, &app, &device.request, Some("000000")).unwrap();
    assert_eq!(approved["approved"], true);
    // Any local connection may list and begin, as before: this mode does not look at windows.
    assert!(
        rig.core
            .pending_pair_view(&app)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn the_mac_keeps_its_denial_text() {
    let mut rig = Rig::new(Approval::SignedApp);
    let app = rig.connect(true, true);
    let window = rig.open_window(&app);
    let device = pair_with(&mut rig, &window, 16, "Test phone");
    rig.notifications(&device.connection).await;
    rig.call(
        &app,
        "device.pair.decide",
        json!({"requestId":device.request,"approve":false}),
    )
    .unwrap();
    let result = pair_result(&mut rig, &device.connection).await;
    assert_eq!(result["reason"], "Denied on Mac");
}

#[tokio::test]
async fn a_refused_system_refuses_the_local_human_and_says_the_app_cannot_prove_itself_yet() {
    let mut rig = Rig::new(Approval::Refused);
    let shell = rig.connect(true, false);
    for (method, params) in [
        ("device.pair.start", json!({})),
        (
            "device.pair.decide",
            json!({"requestId":"r","approve":true}),
        ),
        ("device.revoke", json!({"deviceId":"d"})),
    ] {
        let answer = rig.call(&shell, method, params);
        assert_eq!(kind(&answer), ErrorKind::ForbiddenRole, "{method}");
        assert!(
            text(&answer).contains("cannot yet prove itself"),
            "{method}"
        );
    }
    assert!(
        !rig.core
            .capabilities()
            .contains(&"pairing-code".to_string())
    );
}

#[tokio::test]
async fn a_shipped_engine_has_no_hook_and_does_not_offer_the_code_on_any_system() {
    let directory = std::env::temp_dir().join(format!("soyeht-pairing-prod-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let core = Core::open(&directory).unwrap();
    assert_eq!(core.approval(), Approval::for_this_system());
    assert!(!core.approval().asks_for_code());
    assert!(!core.capabilities().contains(&"pairing-code".to_string()));
    drop(core);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn a_paired_device_and_a_chief_cannot_manage_devices_in_the_person_s_code_mode() {
    let mut rig = Rig::new(Approval::PersonsCode);
    let (outbox, _receiver) = outbox::channel();
    let phone = rig.core.connect(false, false, outbox).unwrap();
    rig.core.clients.get_mut(&phone).unwrap().actor = Some(soyeht_protocol::Actor {
        role: Role::HumanDevice,
        client: "phone".into(),
        device_id: Some("device-1".into()),
        pane_id: None,
    });
    assert_eq!(
        kind(&rig.call(&phone, "device.pair.start", json!({}))),
        ErrorKind::ForbiddenRole
    );
    assert_eq!(
        kind(&rig.call(
            &phone,
            "device.pair.decide",
            json!({"requestId":"r","approve":true})
        )),
        ErrorKind::ForbiddenRole
    );
    assert_eq!(
        kind(&rig.call(&phone, "device.revoke", json!({"deviceId":"device-2"}))),
        ErrorKind::ForbiddenRole
    );
}
