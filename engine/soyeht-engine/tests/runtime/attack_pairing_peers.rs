//! Where a pairing may come from (ADR 0164), against the real engine program: a program of the
//! same user connects to the engine's TCP listener from loopback, with whatever headers it likes,
//! and is refused; the log says why with a kind only. The rules that need a tailnet are shown
//! with the fixtures of `test-pairing-peers` (unit tests of the engine cover each rule).
use super::attack_pairing::{address_of, begin, kind};
use super::terminal_hooks_and_phone::Phone;
use super::*;
use ed25519_dalek::SigningKey;
use tokio_tungstenite::tungstenite::{client, client::IntoClientRequest as _};

const MARKER: &str = "test-pairing-approval";
const PEERS_MARKER: &str = "test-pairing-peers";

/// A rig that asks for the person's code, and the address of its TCP listener.
/// The client that opened the window stays with it: the window belongs to that connection.
fn rig_and_window(peers: Option<&str>) -> (Rig, Client, Value) {
    let rig = Rig::new();
    fs::write(rig.path.join(MARKER), "persons-code").unwrap();
    if let Some(peers) = peers {
        fs::write(rig.path.join(PEERS_MARKER), peers).unwrap();
    }
    let mut app = rig.client();
    let window = app.request("device.pair.start", json!({"idempotencyKey":"window"}));
    assert!(window.get("error").is_none(), "{window}");
    let window = window["result"].clone();
    (rig, app, window)
}

/// The lines of the engine log that name `event`, once there is one: the engine writes its log
/// through a thread of its own, a moment after the answer that caused the line.
fn log_lines(rig: &Rig, event: &str) -> Vec<String> {
    eventually("the engine log names the refusal", || {
        let lines: Vec<String> = fs::read_to_string(rig.path.join("engine.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains(event))
            .map(str::to_owned)
            .collect();
        if lines.is_empty() {
            Err("no line yet".to_owned())
        } else {
            Ok(lines)
        }
    })
}

#[test]
fn a_program_on_loopback_is_refused_whatever_headers_it_sends() {
    // No peers marker: the engine's own rules, with no tailnet listener behind this TCP port.
    let (rig, _app, window) = rig_and_window(None);
    let address = address_of(&window);
    let code = window["code"].as_str().unwrap();
    let key = SigningKey::from_bytes(&[41; 32]);
    for (index, headers) in [
        &[][..],
        &[("X-Forwarded-For", "100.100.0.2")][..],
        &[
            ("Forwarded", "for=100.100.0.2;proto=http"),
            ("X-Real-IP", "100.100.0.2"),
            ("X-Forwarded-For", "100.64.0.9, 100.100.0.2"),
        ][..],
    ]
    .into_iter()
    .enumerate()
    {
        let stream = std::net::TcpStream::connect(&address).unwrap();
        stream
            .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
            .unwrap();
        let mut request = format!("ws://{address}/").into_client_request().unwrap();
        for (name, value) in headers {
            request.headers_mut().insert(*name, value.parse().unwrap());
        }
        let (socket, _) = client(request, stream).unwrap();
        let mut phone = Phone::from_socket(socket);
        let answer = begin(&mut phone, code, &key, &format!("headers-{index}"));
        assert_eq!(kind(&answer), "unauthorized", "{answer}");
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(message.contains("through the tailnet"), "{message}");
    }
    // Nothing waits for the person, and the log says why with a kind only.
    let mut app = rig.client();
    let (_, parts) = app.snapshot();
    assert!(
        !parts
            .iter()
            .any(|part| part.to_string().contains("device_pair"))
    );
    let lines = log_lines(&rig, "engine.pair_refused");
    for line in lines {
        assert!(line.contains(r#""kind":"listener""#), "{line}");
        assert!(
            !line.contains("127.0.0.1") && !line.contains("100."),
            "{line}"
        );
    }
}

#[test]
fn a_proxy_protocol_line_is_not_read_as_an_identity() {
    let (rig, _app, window) = rig_and_window(None);
    let address = address_of(&window);
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    stream
        .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
        .unwrap();
    stream
        .write_all(b"PROXY TCP4 100.100.0.2 100.100.0.1 40000 80\r\n")
        .unwrap();
    let outcome = client(format!("ws://{address}/"), stream);
    assert!(
        outcome.is_err(),
        "the engine took a PROXY line for a WebSocket"
    );
    // The engine is still there, and still refuses a real begin from loopback.
    let key = SigningKey::from_bytes(&[42; 32]);
    let mut phone = Phone::connect(&address);
    let answer = begin(
        &mut phone,
        window["code"].as_str().unwrap(),
        &key,
        "after-proxy",
    );
    assert_eq!(kind(&answer), "unauthorized", "{answer}");
    drop(rig);
}

#[test]
fn tailscale_that_cannot_be_asked_or_names_this_computer_refuses_a_phone() {
    for (peers, expected) in [
        ("whois-down", r#""kind":"whois_unavailable""#),
        ("same-node", r#""kind":"same_node""#),
    ] {
        let (rig, _app, window) = rig_and_window(Some(peers));
        let key = SigningKey::from_bytes(&[43; 32]);
        let mut phone = Phone::connect(&address_of(&window));
        let answer = begin(&mut phone, window["code"].as_str().unwrap(), &key, "begin");
        assert_eq!(kind(&answer), "unauthorized", "{peers}: {answer}");
        let lines = log_lines(&rig, "engine.pair_refused");
        assert!(
            lines.iter().any(|line| line.contains(expected)),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .all(|line| !line.contains("node-") && !line.contains("100.")),
            "{lines:?}"
        );
    }
}

#[test]
fn a_phone_that_tailscale_names_begins_a_pairing_and_the_tailnet_is_pinned() {
    let (rig, _app, window) = rig_and_window(Some("phone"));
    let key = SigningKey::from_bytes(&[44; 32]);
    let mut phone = Phone::connect(&address_of(&window));
    let code = window["code"].as_str().unwrap();
    let started = begin(&mut phone, code, &key, "begin")["result"].clone();
    assert!(started["requestId"].is_string(), "{started}");
    // The engine pinned the tailnet at this first pairing.
    let store = Store::open(&rig.path).unwrap();
    let pinned = soyeht_store::pairing_nodes::pinned(&store)
        .unwrap()
        .unwrap();
    assert_eq!(pinned.tailnet, "fixture.example");
}

#[cfg(target_os = "linux")]
#[test]
fn the_engine_marks_itself_non_dumpable_after_it_starts() {
    use std::os::unix::fs::MetadataExt as _;
    let rig = Rig::new();
    let pid = rig.engine.as_ref().unwrap().id();
    // The files of a process that cannot be dumped belong to the superuser.
    let owner = fs::metadata(format!("/proc/{pid}/status")).unwrap().uid();
    assert_eq!(owner, 0, "the engine is dumpable");
}
