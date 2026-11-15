//! Pairing where the person types the code the new device shows (ADR 0164), against the real
//! engine program. The test engine runs the rules of Linux on any system through the
//! `test-pairing-approval` marker; a shipped engine has no marker and keeps refusing where it
//! cannot attest the app. The phone connects over TCP, as a device does; the local clients stand
//! for the person's app and for a program of the same user.
use super::attack_pairing::{address_of, begin, devices, key_text, kind, proof_for};
use super::terminal_hooks_and_phone::Phone;
use super::*;
use ed25519_dalek::SigningKey;

const MARKER: &str = "test-pairing-approval";
/// The phone of these tests connects from loopback; this marker makes the engine's peer rules see
/// it as a node of the tailnet (`attack_pairing_peers` shows the rules themselves).
const PEERS_MARKER: &str = "test-pairing-peers";

fn persons_code_rig() -> Rig {
    let rig = Rig::new();
    fs::write(rig.path.join(MARKER), "persons-code").unwrap();
    fs::write(rig.path.join(PEERS_MARKER), "phone").unwrap();
    rig
}

/// A local client that has asked for events, and everything the engine sent it.
struct Local {
    client: Client,
    sent: Vec<String>,
}

impl Local {
    fn new(rig: &Rig) -> Self {
        Self {
            client: rig.subscribed_client(),
            sent: vec![],
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let answer = self.client.request(method, params);
        self.sent.push(answer.to_string());
        answer
    }

    fn text(frame: &Frame) -> String {
        match frame {
            Frame::Json(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            other => format!("{other:?}"),
        }
    }

    /// A snapshot, keeping every frame that came before it and every part of it.
    fn snapshot(&mut self) -> Vec<Value> {
        self.sent
            .extend(self.client.messages.iter().map(Self::text));
        let (result, parts) = self.client.snapshot();
        self.sent.push(result.to_string());
        self.sent.extend(parts.iter().map(Value::to_string));
        parts
    }

    /// How many pairing requests a fresh snapshot lists to this connection.
    fn listed_requests(&mut self) -> usize {
        self.snapshot()
            .iter()
            .filter(|part| part["section"] == "requests")
            .flat_map(|part| part["data"].as_array().unwrap().clone())
            .filter(|item| item["kind"] == "device_pair")
            .count()
    }

    /// Everything this connection was sent so far: the answers and every other frame.
    fn all_frames(&self) -> Vec<String> {
        let mut frames = self.sent.clone();
        frames.extend(self.client.messages.iter().map(Self::text));
        frames
    }
}

struct Request {
    phone: Phone,
    id: Value,
    code: String,
}

/// A window of `local` and a device that has proved a request on it.
fn request_on_new_window(local: &mut Local, seed: u8) -> Request {
    let window = local.request(
        "device.pair.start",
        json!({"idempotencyKey":format!("window-{seed}")}),
    );
    assert!(window.get("error").is_none(), "{window}");
    let window = window["result"].clone();
    let key = SigningKey::from_bytes(&[seed; 32]);
    let mut phone = Phone::connect(&address_of(&window));
    let window_code = window["code"].as_str().unwrap();
    let started = begin(&mut phone, window_code, &key, &format!("begin-{seed}"));
    let started = started["result"].clone();
    let proof = proof_for(window_code, &key, &started);
    let proved = phone.request(
        "pair.prove",
        json!({"requestId":started["requestId"],"proof":proof,
            "idempotencyKey":format!("prove-{seed}")}),
    );
    assert_eq!(proved["result"]["status"], "pending", "{proved}");
    let code = proved["result"]["approvalCode"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(code.len(), 6, "{code}");
    assert!(code.bytes().all(|byte| byte.is_ascii_digit()), "{code}");
    Request {
        phone,
        id: started["requestId"].clone(),
        code,
    }
}

fn decide(local: &mut Local, request: &Request, code: Option<&str>, key: &str) -> Value {
    let mut params = json!({"requestId":request.id,"approve":true,"idempotencyKey":key});
    if let Some(code) = code {
        params["approvalCode"] = json!(code);
    }
    local.request("device.pair.decide", params)
}

fn wrong_for(code: &str) -> &'static str {
    if code == "000000" { "000001" } else { "000000" }
}

#[test]
fn a_local_client_without_the_code_cannot_approve_and_three_wrong_codes_deny() {
    let rig = persons_code_rig();
    let mut local = Local::new(&rig);
    let mut request = request_on_new_window(&mut local, 31);
    // No code, a made-up one, a malformed one: refused, nothing paired.
    let none = decide(&mut local, &request, None, "d-none");
    assert_eq!(kind(&none), "pairing_code_invalid", "{none}");
    let wrong = wrong_for(&request.code);
    let first = decide(&mut local, &request, Some(wrong), "d-1");
    assert_eq!(kind(&first), "pairing_code_invalid", "{first}");
    // Not six digits: refused, and it costs no try.
    let typo = decide(&mut local, &request, Some("12"), "d-typo");
    assert_eq!(kind(&typo), "pairing_code_invalid", "{typo}");
    let second = decide(&mut local, &request, Some(wrong), "d-2");
    assert_eq!(kind(&second), "pairing_code_invalid", "{second}");
    assert_eq!(devices(&mut local.client), 0);
    let third = decide(&mut local, &request, Some(wrong), "d-3");
    assert_eq!(kind(&third), "pairing_code_invalid", "{third}");
    let denied_text = third["error"]["message"].as_str().unwrap();
    assert!(denied_text.contains("denied"), "{denied_text}");
    let denied = request.phone.read();
    assert_eq!(denied["method"], "pair.result", "{denied}");
    assert_eq!(denied["params"]["approved"], false);
    assert_eq!(denied["params"]["reason"], "Denied on the computer");
    // The real code is too late: the request was denied by the wrong ones.
    let late = decide(&mut local, &request, Some(&request.code.clone()), "d-4");
    assert_eq!(kind(&late), "transition_refused", "{late}");
    assert_eq!(devices(&mut local.client), 0);
}

#[test]
fn the_code_the_phone_shows_approves_the_request_once() {
    let rig = persons_code_rig();
    let mut local = Local::new(&rig);
    let mut request = request_on_new_window(&mut local, 32);
    let typed = format!("{} {}", &request.code[..3], &request.code[3..]);
    let approved = decide(&mut local, &request, Some(&typed), "approve");
    assert_eq!(approved["result"]["approved"], true, "{approved}");
    let result = request.phone.read();
    assert_eq!(result["params"]["approved"], true, "{result}");
    assert!(result["params"]["deviceToken"].is_string());
    assert_eq!(devices(&mut local.client), 1);
    // The same code again, with a new key, is a request already decided.
    let again = decide(
        &mut local,
        &request,
        Some(&request.code.clone()),
        "approve-again",
    );
    assert_eq!(kind(&again), "transition_refused", "{again}");
    assert_eq!(devices(&mut local.client), 1);
}

#[test]
fn a_program_of_the_same_user_decides_nothing_on_a_window_it_did_not_open() {
    let rig = persons_code_rig();
    let mut app = Local::new(&rig);
    let mut shell = Local::new(&rig);
    let request = request_on_new_window(&mut app, 33);
    // It even knows the code (as a program on the phone's own node would), and still cannot
    // decide, deny or see the request.
    let approved = decide(
        &mut shell,
        &request,
        Some(&request.code.clone()),
        "shell-approve",
    );
    assert_eq!(kind(&approved), "forbidden_role", "{approved}");
    let denied = shell.request(
        "device.pair.decide",
        json!({"requestId":request.id,"approve":false,"idempotencyKey":"shell-deny"}),
    );
    assert_eq!(kind(&denied), "forbidden_role", "{denied}");
    assert_eq!(shell.listed_requests(), 0);
    assert_eq!(app.listed_requests(), 1);
    assert_eq!(devices(&mut app.client), 0);
}

#[test]
fn a_local_connection_cannot_pair_as_a_device() {
    let rig = persons_code_rig();
    let mut local = Local::new(&rig);
    let window =
        local.request("device.pair.start", json!({"idempotencyKey":"window"}))["result"].clone();
    // The local endpoint is where the person's own programs connect: no device pairs there.
    let socket = connect(&rig.path.join("engine.sock")).unwrap();
    let mut program = Client {
        socket,
        next: 0,
        messages: vec![],
    };
    let _challenge = Frame::read_from(&mut program.socket).unwrap();
    let key = SigningKey::from_bytes(&[34; 32]);
    let answer = program.request(
        "pair.begin",
        json!({"code":window["code"],"deviceName":"Test phone",
            "devicePublicKey":key_text(&key),"idempotencyKey":"local-begin"}),
    );
    assert_eq!(kind(&answer), "unauthorized", "{answer}");
}

#[test]
fn the_approval_code_is_in_no_frame_no_file_and_no_log_of_the_computer() {
    let rig = persons_code_rig();
    let mut local = Local::new(&rig);
    let mut request = request_on_new_window(&mut local, 35);
    let code = request.code.clone();
    let wrong = decide(&mut local, &request, Some(wrong_for(&code)), "leak-wrong");
    assert_eq!(kind(&wrong), "pairing_code_invalid", "{wrong}");
    let approved = decide(&mut local, &request, Some(&code), "leak-right");
    assert_eq!(approved["result"]["approved"], true, "{approved}");
    request.phone.read();
    local.request("device.list", json!({}));
    local.snapshot();
    // Frames: the answers and notifications, and the snapshot a client reads.
    let frames = local.all_frames();
    assert!(frames.len() > 5);
    for frame in &frames {
        assert!(!frame.contains(&code), "{frame}");
    }
    // Files: the database and its journal, the engine's log, anything else in its directory.
    let mut scanned = 0;
    for entry in fs::read_dir(&rig.path).unwrap().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.starts_with("soyeht.db") || name == "engine.log") {
            continue;
        }
        scanned += 1;
        let bytes = fs::read(&path).unwrap();
        let found = bytes
            .windows(code.len())
            .any(|window| window == code.as_bytes());
        assert!(!found, "{name} holds the approval code");
    }
    assert!(scanned >= 2, "scanned {scanned} files");
}

#[test]
fn a_fifth_window_closes_the_oldest_and_a_device_name_cannot_reorder_text() {
    let rig = persons_code_rig();
    let mut local = Local::new(&rig);
    let windows: Vec<Value> = (0..5)
        .map(|index| {
            local.request(
                "device.pair.start",
                json!({"idempotencyKey":format!("many-{index}")}),
            )["result"]
                .clone()
        })
        .collect();
    let key = SigningKey::from_bytes(&[36; 32]);
    let mut phone = Phone::connect(&address_of(&windows[0]));
    let oldest = begin(
        &mut phone,
        windows[0]["code"].as_str().unwrap(),
        &key,
        "oldest",
    );
    assert_eq!(kind(&oldest), "unauthorized", "{oldest}");
    let newest = begin(
        &mut phone,
        windows[4]["code"].as_str().unwrap(),
        &key,
        "newest",
    );
    assert!(newest.get("error").is_none(), "{newest}");
    let mut phone = Phone::connect(&address_of(&windows[4]));
    let spoofed = phone.request(
        "pair.begin",
        json!({"code":windows[4]["code"],"deviceName":"gpj.evil\u{202E}","idempotencyKey":"bidi",
            "devicePublicKey":key_text(&SigningKey::from_bytes(&[37; 32]))}),
    );
    assert_eq!(kind(&spoofed), "invalid_params", "{spoofed}");
}

#[test]
fn without_the_marker_this_engine_follows_the_rule_of_its_system() {
    // The same program as a shipped engine would be, but for its hooks: no marker, no mode. On
    // macOS an unsigned program cannot be the app and the local human is refused; on Linux the
    // person's code is the rule; on Windows nothing can attest the app yet.
    let rig = Rig::new();
    fs::write(rig.path.join(MARKER), "system").unwrap();
    let mut local = rig.client();
    let started = local.request("device.pair.start", json!({"idempotencyKey":"w"}));
    let (_, parts) = local.snapshot();
    let capabilities: Vec<Value> = parts
        .iter()
        .filter(|part| part["section"] == "capabilities")
        .flat_map(|part| part["data"].as_array().unwrap().clone())
        .collect();
    assert!(!capabilities.is_empty());
    let offers_code = capabilities.iter().any(|name| name == "pairing-code");
    if cfg!(target_os = "linux") {
        assert!(started.get("error").is_none(), "{started}");
        assert!(offers_code);
    } else {
        assert_eq!(kind(&started), "forbidden_role", "{started}");
        assert!(!offers_code);
    }
    if cfg!(windows) {
        assert!(
            started["error"]["message"]
                .as_str()
                .unwrap()
                .contains("cannot yet prove itself")
        );
    }
}

#[test]
fn the_refused_rule_names_a_system_where_the_app_cannot_prove_itself_yet() {
    let rig = Rig::new();
    fs::write(rig.path.join(MARKER), "refused").unwrap();
    let mut local = rig.client();
    let refused = local.request("device.pair.start", json!({"idempotencyKey":"w"}));
    assert_eq!(kind(&refused), "forbidden_role", "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot yet prove itself"),
        "{refused}"
    );
}
