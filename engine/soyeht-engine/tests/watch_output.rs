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
use support::{Rig, wait_for_shell};

/// More than three reads of the supervisor (64 KiB each) and less than the 2 MiB it keeps.
const BURST: usize = 200_000;
/// More than 256 reads of 64 KiB, which a counter of consecutive short reads of one byte cannot
/// hold. No pause: the supervisor keeps 8 MiB per process, so the engine must read as it goes.
const LONG: usize = 20_000_000;
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

/// Prints `bytes` of `x` in a new pane and returns how long the whole of it took to arrive, once
/// the engine has run again. With `stopped` the engine does not run while the burst is printed, so
/// the supervisor takes it all in before the engine reads.
fn burst(bytes: usize, stopped: Option<Duration>, limit: Duration) -> (Rig, Vec<u8>, Duration) {
    let rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let mut watcher = rig.client();
    let (info, _) = watcher.tail(&pane);
    let pane_ref = info["binaryPaneRef"].as_u64().unwrap();
    // The burst is the only output once the shell is up: a login shell on a busy machine can take
    // seconds to start, and what is typed before it reads the terminal is lost.
    wait_for_shell(&mut client, &mut watcher, &pane, pane_ref);

    let pause = if stopped.is_some() { "sleep 0.5; " } else { "" };
    let command = format!("{pause}head -c {bytes} /dev/zero | tr '\\0' x; echo DO\"\"NE\n");
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(command),"idempotencyKey":"burst"}),
    );
    if let Some(stopped) = stopped {
        let engine = rig.engine.as_ref().unwrap().id();
        signal(engine, "-STOP");
        thread::sleep(stopped);
        signal(engine, "-CONT");
    }
    let started = Instant::now();
    let mut received = Vec::new();
    while !String::from_utf8_lossy(&received[received.len().saturating_sub(1_024)..])
        .contains("DONE")
    {
        assert!(
            started.elapsed() < limit,
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
    (rig, received, started.elapsed())
}

fn engine_log(rig: &Rig) -> String {
    fs::read_to_string(rig.path.join("engine.log")).unwrap_or_default()
}

#[test]
fn a_burst_longer_than_one_read_arrives_whole_without_waiting_for_the_sweep() {
    // The command waits half a second before it prints and the engine does not run for a second:
    // the supervisor takes the whole burst in before the engine reads, and the one notice it sent
    // for it is read when the burst is over; one read cannot return all of it. The stop is well
    // under the two seconds after which a sweep counts output nobody announced as missed.
    let (rig, received, took) = burst(
        BURST,
        Some(Duration::from_millis(1_000)),
        Duration::from_secs(10),
    );
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
    assert!(
        !engine_log(&rig).contains("engine.supervisor_watch_missed"),
        "a sweep found output nobody announced"
    );
}

#[test]
fn a_process_that_stays_behind_for_hundreds_of_reads_does_not_stop_the_engine() {
    let (rig, received, _) = burst(LONG, None, Duration::from_secs(60));
    assert!(
        received.len() >= 1_000_000,
        "the burst arrived short: {} bytes",
        received.len()
    );
    let log = engine_log(&rig);
    assert!(
        !log.contains("crash.report_written") && !log.contains("overflow"),
        "the engine failed on a long burst"
    );
}
