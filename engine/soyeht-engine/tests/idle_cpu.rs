//! Idle cost of a running engine. An engine with no client connected and
//! agents waiting for work must spend almost no CPU: it does work on events,
//! not on every tick. The deterministic regression guard (no SQL and no agent
//! state clone on an idle tick) lives in `agents::idle`; this bench measures
//! the whole process.
//!
//! Run it with the release profile for numbers that match the shipped engine:
//! `cargo test --release -p soyeht-engine --test idle_cpu -- --ignored --nocapture`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

mod support;
use support::*;

const SETTLE: Duration = Duration::from_secs(3);
const WINDOW: Duration = Duration::from_secs(10);

/// CPU seconds the process has used, from `ps` (`[[dd-]hh:]mm:ss[.ff]`).
fn cpu_seconds(pid: u32) -> f64 {
    let output = Command::new("ps")
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

#[test]
#[ignore = "bench: measures wall-clock CPU; run explicitly"]
fn idle_engine_with_idle_agents_costs_almost_no_cpu() {
    let rig = Rig::new();
    let (mut client, _first) = rig.agent();
    let (_, _second) = rig.agent();
    client.create(&rig);
    drop(client);
    thread::sleep(SETTLE);
    let pid = rig.engine.as_ref().unwrap().id();
    let before = cpu_seconds(pid);
    let started = Instant::now();
    thread::sleep(WINDOW);
    let used = cpu_seconds(pid) - before;
    let percent = used / started.elapsed().as_secs_f64() * 100.0;
    eprintln!("idle engine cpu: {used:.2}s over {WINDOW:?} = {percent:.1}% of one core");
    assert!(percent < 5.0, "an idle engine used {percent:.1}% of a core");
}
