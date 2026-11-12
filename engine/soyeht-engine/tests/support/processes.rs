//! The processes a test starts: waiting for a shell that is still starting, and what a process
//! has used.
use super::Client;
use serde_json::json;
use soyeht_protocol::frame::Frame;
use std::{
    thread,
    time::{Duration, Instant},
};

/// Types a marker until the shell echoes it back from its output.
pub fn wait_for_shell(client: &mut Client, watcher: &mut Client, pane: &str, pane_ref: u64) {
    watcher
        .socket
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen = Vec::new();
    for attempt in 0.. {
        assert!(Instant::now() < deadline, "the shell never answered");
        client.call(
            "pane.input",
            json!({"paneId":pane,"bytes":base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "echo RE\"\"ADY\n"),
                "idempotencyKey":format!("ready-{attempt}")}),
        );
        while let Ok(Some(frame)) = Frame::read_from(&mut watcher.socket) {
            if let Frame::Terminal {
                pane_ref: found,
                bytes,
                ..
            } = frame
                && u64::from(found) == pane_ref
            {
                seen.extend(bytes);
            }
        }
        if String::from_utf8_lossy(&seen).contains("READY\r\n") {
            break;
        }
    }
    // Everything the marker caused has been read.
    thread::sleep(Duration::from_millis(300));
    while let Ok(Some(_)) = Frame::read_from(&mut watcher.socket) {}
    watcher
        .socket
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
}

/// A terminal pane in a new workspace of `rig` and a second client that watches its output:
/// both clients, the workspace, the pane and the pane's reference in the frames.
pub fn watched_shell(rig: &super::Rig) -> (Client, Client, String, String, u64) {
    let mut client = rig.client();
    let (workspace, pane) = client.create(rig);
    let mut watcher = rig.client();
    let (info, _) = watcher.tail(&pane);
    let pane_ref = info["binaryPaneRef"].as_u64().unwrap();
    (client, watcher, workspace, pane, pane_ref)
}

/// Lets a started shell settle (its prompt and the sweep of what it printed are over) and reads
/// away what the watcher was sent, so the next frames are the ones a test causes.
pub fn settle_shell(watcher: &mut Client, settle: Duration) {
    thread::sleep(settle);
    watcher
        .socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    while let Ok(Some(_)) = Frame::read_from(&mut watcher.socket) {}
    watcher
        .socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
}

/// Types the `key`th letter of the alphabet into `pane` as one request and waits for the frame of
/// the watcher that carries its echo; how long that took.
pub fn type_letter_and_wait_for_echo(
    client: &mut Client,
    watcher: &mut Client,
    (pane, pane_ref): (&str, u64),
    key: usize,
) -> Duration {
    let letter = char::from(b'a' + u8::try_from(key % 26).unwrap());
    let started = Instant::now();
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":base64::Engine::encode(&base64::engine::general_purpose::STANDARD, letter.to_string()),
            "idempotencyKey":format!("key-{key}")}),
    );
    loop {
        let frame = Frame::read_from(&mut watcher.socket)
            .unwrap()
            .expect("the echo");
        if let Frame::Terminal {
            pane_ref: found,
            bytes,
            ..
        } = frame
            && u64::from(found) == pane_ref
            && bytes.contains(&(letter as u8))
        {
            return started.elapsed();
        }
    }
}

/// CPU seconds the process has used, from `ps` (`[[dd-]hh:]mm:ss[.ff]`, whole seconds on Linux).
pub fn cpu_seconds(pid: u32) -> f64 {
    let output = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let text = text.trim();
    let (days, clock) = text.split_once('-').unwrap_or(("0", text));
    let seconds = clock.split(':').fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap()
    });
    days.parse::<f64>().unwrap() * 86_400.0 + seconds
}
