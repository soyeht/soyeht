//! Terminal output keeps flowing while a slow external source blocks (ADR 0079).
//! The fixture is a POSIX shell script that sleeps, so the check runs where there is a
//! shell; the lock rule itself is the same on every system.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    thread,
    time::{Duration, Instant},
};
use support::{Rig, Ticker, longest_gap};

/// How long the fake usage source blocks. Before ADR 0079 the engine held its
/// Core lock for all of it, and no terminal output reached any client.
const SLOW_SOURCE: Duration = Duration::from_secs(5);
/// A window with no slow work, measured first on the same rig and load.
const CONTROL_WINDOW: Duration = Duration::from_secs(2);
/// A gap under the slow source is allowed this many times the control gap,
/// for scheduling noise on a loaded machine.
const CONTROL_FACTOR: u32 = 4;
/// The floor of the allowed gap, so a very quiet control does not make the
/// bound flaky.
const GAP_FLOOR: Duration = Duration::from_millis(750);
/// The ceiling of the allowed gap: half the blocking source, so the old
/// behaviour (one gap as long as the source) can never pass.
const GAP_CEILING: Duration = Duration::from_millis(2_500);

fn install_slow_usage_source(rig: &Rig) {
    let directory = rig.usage_readers();
    fs::create_dir_all(&directory).unwrap();
    let source = directory.join("second-source");
    fs::write(
        &source,
        format!("#!/bin/sh\nexec sleep {}\n", SLOW_SOURCE.as_secs()),
    )
    .unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn terminal_output_keeps_flowing_while_a_usage_source_blocks() {
    let rig = Rig::new();
    install_slow_usage_source(&rig);
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let ticker = Ticker::start(&rig, &mut client, &pane);
    // Let the loop start printing before the control window opens.
    let warmed = Instant::now() + Duration::from_millis(500);
    while Instant::now() < warmed {
        thread::sleep(Duration::from_millis(10));
    }
    let control_start = Instant::now();
    thread::sleep(CONTROL_WINDOW);
    let control_end = Instant::now();

    let slow_start = Instant::now();
    let refresh = thread::spawn({
        let mut caller = rig.client();
        move || {
            caller.request(
                "usage.refresh",
                json!({"provider":"codex","idempotencyKey":"slow-usage"}),
            )
        }
    });
    // Another connection's request is answered while the source blocks.
    thread::sleep(Duration::from_millis(500));
    let status_started = Instant::now();
    let status = client.call("engine.status", json!({}));
    let status_latency = status_started.elapsed();
    let reply: Value = refresh.join().unwrap();
    let slow_end = Instant::now();
    let arrivals = ticker.finish();

    let refresh_duration = slow_end - slow_start;
    let control_gap = longest_gap(&arrivals, control_start, control_end);
    let slow_gap = longest_gap(&arrivals, slow_start, slow_end);
    let allowed = (control_gap * CONTROL_FACTOR).clamp(GAP_FLOOR, GAP_CEILING);
    eprintln!(
        "core lock: refresh={refresh_duration:?} control_gap={control_gap:?} slow_gap={slow_gap:?} allowed={allowed:?} status_latency={status_latency:?} frames={}",
        arrivals.len()
    );
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(reply["result"]["providers"][0]["state"], "unavailable");
    assert!(
        refresh_duration >= SLOW_SOURCE - Duration::from_millis(500),
        "the fake usage source did not block: {refresh_duration:?}"
    );
    assert!(status["generation"].is_number());
    assert!(
        status_latency < GAP_CEILING,
        "another connection waited behind the usage source: {status_latency:?}"
    );
    assert!(
        slow_gap <= allowed,
        "terminal output stalled for {slow_gap:?} behind the usage source (control {control_gap:?})"
    );
}
