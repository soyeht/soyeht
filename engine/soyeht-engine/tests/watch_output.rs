//! The supervisor tells the engine when a process has output, and the engine reads all of a burst
//! that is longer than one read (ADR 0147, step 1a).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use soyeht_protocol::frame::Frame;
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use support::Rig;

/// More than three reads of the supervisor (64 KiB each) and less than the 2 MiB it keeps.
const BURST: usize = 200_000;
/// The sweep reads every process once a second, and one read returns 64 KiB: a burst whose tail
/// depended on it would arrive a second late or more. Heard from notices it takes a few tens of
/// milliseconds.
const WITHIN: Duration = Duration::from_millis(600);

fn signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status()
        .unwrap();
    assert!(status.success(), "kill {signal} {pid} failed");
}

#[test]
fn a_burst_longer_than_one_read_arrives_whole_without_waiting_for_the_sweep() {
    let rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let mut watcher = rig.client();
    let (info, _) = watcher.tail(&pane);
    let pane_ref = info["binaryPaneRef"].as_u64().unwrap();
    // Let the shell settle, so the burst is the only output.
    thread::sleep(Duration::from_millis(500));

    let command = format!("sleep 1; head -c {BURST} /dev/zero | tr '\\0' x; echo DO\"\"NE\n");
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(command),"idempotencyKey":"burst"}),
    );
    // The command waits a second before it prints. The engine does not run meanwhile, so the
    // supervisor takes the whole burst in before the engine reads, and the one notice it sent for
    // it is read when the burst is over: one read cannot return all of it.
    let engine = rig.engine.as_ref().unwrap().id();
    signal(engine, "-STOP");
    thread::sleep(Duration::from_millis(1_800));
    signal(engine, "-CONT");
    let started = Instant::now();
    let mut received = Vec::new();
    while !String::from_utf8_lossy(&received).contains("DONE") {
        assert!(
            started.elapsed() < WITHIN * 10,
            "the burst did not arrive: {} bytes",
            received.len()
        );
        match Frame::read_from(&mut watcher.socket).unwrap() {
            Some(Frame::Terminal {
                pane_ref: found,
                bytes,
                ..
            }) if u64::from(found) == pane_ref => received.extend(bytes),
            Some(_) => {}
            None => panic!("engine closed the watcher"),
        }
    }
    let took = started.elapsed();
    assert!(
        received.iter().filter(|byte| **byte == b'x').count() >= BURST,
        "the burst arrived short: {} bytes",
        received.len()
    );
    assert!(
        took < WITHIN,
        "the burst took {took:?}: its tail waited for the sweep"
    );

    // Past a sweep or two, which read every process once more: they found nothing the notices
    // had not announced.
    thread::sleep(Duration::from_millis(2_500));
    let log = fs::read_to_string(rig.path.join("engine.log")).unwrap_or_default();
    assert!(
        !log.contains("engine.supervisor_watch_missed"),
        "a sweep found output nobody announced"
    );
}
