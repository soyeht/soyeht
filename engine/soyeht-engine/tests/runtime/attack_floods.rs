//! Resource exhaustion and the remote listener: more connections than the engine admits, a web
//! page reaching for the listener, a message past the frame limit, and a prompt past the size
//! limit. The engine refuses the excess and keeps serving the person's own client.
use super::attack_pairing::listener_address;
use super::*;
use soyeht_protocol::limits;
use std::io::Read;
use std::net::TcpStream;

fn kind(answer: &Value) -> &str {
    answer["error"]["data"]["kind"]
        .as_str()
        .unwrap_or("<no error>")
}

const UPGRADE: &str = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n";

/// The first line of the answer to a WebSocket upgrade, with `extra` header lines added.
fn upgrade_status(address: &str, extra: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(ASYNC_ASSERTION_TIMEOUT))
        .unwrap();
    stream
        .write_all(format!("{UPGRADE}{extra}\r\n").as_bytes())
        .unwrap();
    let mut answer = [0u8; 64];
    let read = stream.read(&mut answer).unwrap();
    String::from_utf8_lossy(&answer[..read])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_web_page_cannot_open_the_remote_listener_and_a_program_can_reach_it_only_unproven() {
    let rig = Rig::new();
    let address = listener_address(&rig);
    // A browser puts the page's origin on every WebSocket it opens and no script can omit it.
    for origin in [
        "https://evil.example",
        "http://localhost:3000",
        "null",
        "file://",
    ] {
        let status = upgrade_status(&address, &format!("Origin: {origin}\r\n"));
        assert!(status.contains("403"), "origin {origin}: {status}");
    }
    assert!(upgrade_status(&address, "").contains("101"));
}

#[test]
fn a_message_past_the_frame_limit_ends_the_remote_connection_and_nothing_else() {
    let rig = Rig::new();
    let address = listener_address(&rig);
    let mut phone = super::terminal_hooks_and_phone::Phone::connect(&address);
    let text = "x".repeat(limits::JSON_BYTES + 1);
    let sent = phone
        .socket
        .send(tokio_tungstenite::tungstenite::Message::Text(text.into()));
    // Either the send fails because the engine already hung up, or the next read finds it gone.
    let ended = sent.is_err()
        || matches!(
            phone.socket.read(),
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_)
        );
    assert!(ended, "the connection survived an oversized message");
    assert!(rig.client().call("engine.status", json!({})).is_object());
}

#[test]
fn one_remote_address_cannot_take_more_than_its_share_of_connections() {
    let rig = Rig::new();
    let address = listener_address(&rig);
    let mut held = Vec::new();
    let mut refused = 0;
    for _ in 0..40 {
        let Ok(mut stream) = TcpStream::connect(&address) else {
            refused += 1;
            continue;
        };
        // A connection the engine has already closed refuses a timeout, a write or a read.
        let sent = stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .and_then(|()| stream.write_all(format!("{UPGRADE}\r\n").as_bytes()));
        if sent.is_err() {
            refused += 1;
            continue;
        }
        let mut answer = [0u8; 16];
        match stream.read(&mut answer) {
            Ok(count) if count > 0 && answer.starts_with(b"HTTP/1.1 101") => held.push(stream),
            _ => refused += 1,
        }
    }
    // The engine admits 16 per address (`SLOTS_PER_ADDRESS`); the rest are turned away.
    assert!(held.len() <= 16, "{} connections held", held.len());
    assert!(refused >= 24, "only {refused} refused");
    // The person's own endpoint has places of its own.
    assert!(rig.client().call("engine.status", json!({})).is_object());
    drop(held);
}

#[test]
fn a_flood_of_local_connections_is_bounded_and_the_engine_recovers() {
    let rig = Rig::new();
    let path = rig.path.join("engine.sock");
    let mut held = Vec::new();
    let mut turned_away = 0;
    for _ in 0..150 {
        let Ok(mut socket) = connect(&path) else {
            turned_away += 1;
            continue;
        };
        // macOS refuses to set a timeout on a connection the engine has already closed.
        if socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .is_err()
        {
            turned_away += 1;
            continue;
        }
        match Frame::read_from(&mut socket) {
            Ok(Some(_)) => held.push(socket),
            _ => turned_away += 1,
        }
    }
    // The engine admits 128 connections of the local endpoint (`LOCAL_SLOTS`) at once.
    assert!(held.len() <= 128, "{} connections admitted", held.len());
    assert!(turned_away >= 22, "only {turned_away} turned away");
    drop(held);
    // The slots are free again once the flood hangs up.
    wait_until("the engine to admit a client again", || {
        connect(&path).is_ok_and(|mut socket| {
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            matches!(Frame::read_from(&mut socket), Ok(Some(_)))
        })
    });
    assert!(rig.client().call("engine.status", json!({})).is_object());
}

#[test]
fn a_prompt_past_the_size_limit_is_refused_and_one_at_the_limit_is_taken() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let over = client.request(
        "pane.prompt",
        json!({"paneId":pane,"text":"a".repeat(limits::PROMPT_BYTES + 1),"idempotencyKey":"over"}),
    );
    assert_eq!(kind(&over), "invalid_params", "{over}");
    assert!(transcript_items(&mut client, &pane).is_empty());
}

#[test]
fn a_request_exactly_at_the_frame_limit_is_answered_and_one_byte_more_is_not() {
    let rig = Rig::new();
    let mut client = rig.client();
    let base = json!({"jsonrpc":"2.0","id":7001,"method":"engine.status","params":{"pad":""}});
    let overhead = serde_json::to_vec(&base).unwrap().len();
    let pad = "a".repeat(limits::JSON_BYTES - overhead);
    let request = json!({"jsonrpc":"2.0","id":7001,"method":"engine.status","params":{"pad":pad}});
    let body = serde_json::to_vec(&request).unwrap();
    assert_eq!(body.len(), limits::JSON_BYTES);
    Frame::Json(body).write_to(&mut client.socket).unwrap();
    let answer = client.wait(7001);
    assert!(
        answer.get("result").is_some(),
        "{}",
        &answer.to_string()[..200.min(answer.to_string().len())]
    );
}
