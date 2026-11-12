//! An idle engine wakes once in a while, not on every tick (ADR 0147, step 1b).
//!
//! The acceptance of the idle cost is by counts, which do not depend on the host: the context
//! switches per second of the engine, with no pane and with idle shells. A tick that runs every
//! 10 ms costs hundreds of them a second whatever the machine does, so the limits below hold on a
//! busy host and still tell the tick that sleeps from the tick that polls. The same measurement is
//! printed for the supervisor, whose own 10 ms exit poll is other work.
//!
//! Run it with the release profile for the numbers of the shipped engine:
//! `cargo test --release -p soyeht-engine --test idle_wakeups -- --nocapture`
#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use std::{
    thread,
    time::{Duration, Instant},
};

mod support;
use support::*;

/// Time for the shells to start and the engine to read what they print.
const SETTLE: Duration = Duration::from_secs(6);
/// How long the switches are counted.
const WINDOW: Duration = Duration::from_secs(6);
/// The longest the median echo of a key may take in an idle shell.
const ECHO_LIMIT: Duration = Duration::from_millis(400);
/// The most an idle engine may switch contexts per second. The tick of 10 ms made 450 to 560 of
/// them with no pane and with a few shells; a tick that wakes for a deadline or an event makes a
/// few tens.
const ENGINE_LIMIT_PER_SECOND: f64 = 150.0;

/// The context switches the process has made so far, voluntary or not.
fn context_switches(pid: u32) -> u64 {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        status
            .lines()
            .filter(|line| line.contains("ctxt_switches"))
            .map(|line| {
                line.split_whitespace()
                    .last()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
            })
            .sum()
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: an all-zero `proc_taskinfo` is a valid value of the plain C struct.
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = i32::try_from(std::mem::size_of::<libc::proc_taskinfo>()).unwrap();
        // SAFETY: `info` is a writable buffer of `size` bytes, as the call requires.
        let written = unsafe {
            libc::proc_pidinfo(
                i32::try_from(pid).unwrap(),
                libc::PROC_PIDTASKINFO,
                0,
                std::ptr::from_mut(&mut info).cast(),
                size,
            )
        };
        assert_eq!(written, size, "proc_pidinfo failed for {pid}");
        u64::try_from(info.pti_csw).unwrap()
    }
}

/// Starts `count` more shells in `workspace` and waits until each has a process.
fn start_shells(client: &mut Client, workspace: &str, count: usize) {
    let panes: Vec<String> = (0..count)
        .map(|index| {
            let created = client.call(
                "pane.create",
                json!({"workspaceId":workspace,"mode":"terminal",
                    "idempotencyKey":format!("shell-{index}")}),
            );
            created["paneId"].as_str().unwrap().to_owned()
        })
        .collect();
    wait_until("the shells started", || {
        panes.iter().all(|pane| !client.pane(pane)["pid"].is_null())
    });
}

/// Context switches a second of the engine and of the supervisor over [`WINDOW`], with `panes`
/// idle shells in one workspace.
fn idle_switches(panes: usize) -> (f64, f64) {
    let rig = Rig::new();
    if panes > 0 {
        let mut client = rig.client();
        let (workspace, _) = client.create(&rig);
        start_shells(&mut client, &workspace, panes - 1);
    }
    thread::sleep(SETTLE);
    if panes > 0 {
        let mut client = rig.client();
        let alive = client
            .section("panes")
            .iter()
            .filter(|pane| !pane["pid"].is_null() && pane["state"] != "dead")
            .count();
        assert_eq!(alive, panes, "every shell is running when the count starts");
    }
    let engine = rig.engine.as_ref().unwrap().id();
    let supervisor = rig.ptyd.id();
    let before = (
        context_switches(engine),
        context_switches(supervisor),
        cpu_seconds(engine),
        cpu_seconds(supervisor),
    );
    let started = Instant::now();
    thread::sleep(WINDOW);
    let seconds = started.elapsed().as_secs_f64();
    let engine_rate = (context_switches(engine) - before.0) as f64 / seconds;
    let supervisor_rate = (context_switches(supervisor) - before.1) as f64 / seconds;
    // The CPU depends on the host: it is printed for a meter on a quiet one, never asserted.
    let engine_cpu = (cpu_seconds(engine) - before.2) / seconds * 100.0;
    let supervisor_cpu = (cpu_seconds(supervisor) - before.3) / seconds * 100.0;
    eprintln!(
        "{panes} idle shells: engine {engine_rate:.0} context switches/s ({engine_cpu:.1}% of a \
         core), supervisor {supervisor_rate:.0}/s ({supervisor_cpu:.1}%)"
    );
    (engine_rate, supervisor_rate)
}

#[test]
fn an_engine_with_no_pane_does_not_wake_on_every_tick() {
    let (engine, _) = idle_switches(0);
    assert!(
        engine < ENGINE_LIMIT_PER_SECOND,
        "an engine with no pane made {engine:.0} context switches a second"
    );
}

#[test]
fn an_engine_with_idle_shells_does_not_wake_on_every_tick() {
    let (engine, _) = idle_switches(5);
    assert!(
        engine < ENGINE_LIMIT_PER_SECOND,
        "an engine with 5 idle shells made {engine:.0} context switches a second"
    );
}

/// Types one key at a time into an idle shell, a pause apart so that the tick is asleep, and
/// returns how long each took from the request to the frame that carries its echo.
fn echo_latencies(keys: usize) -> Vec<Duration> {
    let rig = Rig::new();
    let (mut client, mut watcher, workspace, pane, pane_ref) = watched_shell(&rig);
    start_shells(&mut client, &workspace, 4);
    wait_for_shell(&mut client, &mut watcher, &pane, pane_ref);
    settle_shell(&mut watcher, Duration::from_secs(3));
    let mut latencies = Vec::new();
    for key in 0..keys {
        latencies.push(type_letter_and_wait_for_echo(
            &mut client,
            &mut watcher,
            (&pane, pane_ref),
            key,
        ));
        thread::sleep(Duration::from_millis(250));
    }
    latencies.sort();
    latencies
}

#[test]
fn the_echo_of_a_key_in_an_idle_shell_does_not_wait_for_the_tick() {
    let latencies = echo_latencies(40);
    let at =
        |numerator: usize| latencies[(latencies.len() * numerator / 100).min(latencies.len() - 1)];
    eprintln!(
        "echo of a key in an idle shell (5 idle shells): p50 {:?}, p90 {:?}, p99 {:?}, max {:?}",
        at(50),
        at(90),
        at(99),
        latencies.last().unwrap()
    );
    // The echo is read when the supervisor says it has output. An echo that waited for the tick's
    // ceiling (a second) or for a sweep would put the median there; a busy host does not.
    assert!(
        at(50) < ECHO_LIMIT,
        "the median echo of a key took {:?}",
        at(50)
    );
}
