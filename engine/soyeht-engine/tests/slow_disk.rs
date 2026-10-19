//! A slow disk must not stall the engine (ADR 0079, ADR 0095). Terminal output is flushed to
//! the pane's segment file before the supervisor may drop it. On a busy machine one flush takes
//! seconds (a full flush of the drive on macOS), and the tick used to wait for it with the Core
//! lock held, so output of every pane and every request on every connection waited too.
//! The test engine pauses before each flush (`SOYEHT_TEST_SLOW_DISK_MS`).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use std::{
    thread,
    time::{Duration, Instant},
};
use support::{Rig, Ticker, longest_gap};

/// How long each flush takes. The old tick waited this long for every chunk of output.
const FLUSH: &str = "1500";
/// How long the engine is watched.
const WINDOW: Duration = Duration::from_secs(5);
/// The longest stretch a watcher may get no terminal frame, or a request no reply. With the
/// flush in the tick both are as long as a flush; without it, a scheduling delay.
const STALL_CEILING: Duration = Duration::from_millis(700);

#[test]
fn a_slow_disk_does_not_stall_terminal_output_or_requests() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_SLOW_DISK_MS", FLUSH)]);
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let ticker = Ticker::start(&rig, &mut client, &pane);

    let started = Instant::now();
    let mut slowest_request = Duration::ZERO;
    let mut prober = rig.client();
    while started.elapsed() < WINDOW {
        let asked = Instant::now();
        prober.call("engine.status", json!({}));
        slowest_request = slowest_request.max(asked.elapsed());
        thread::sleep(Duration::from_millis(100));
    }
    let ended = Instant::now();
    let arrivals = ticker.finish();
    let silence = longest_gap(&arrivals, started, ended);

    eprintln!(
        "{} terminal frames in {WINDOW:?}; longest silence {silence:?}; \
         slowest engine.status {slowest_request:?}",
        arrivals.len()
    );
    assert!(
        silence < STALL_CEILING,
        "no terminal frame for {silence:?}: the tick waited for the disk"
    );
    assert!(
        slowest_request < STALL_CEILING,
        "a request waited {slowest_request:?}: the tick held the Core lock across a disk flush"
    );
}
