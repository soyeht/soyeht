//! What a key typed into a terminal pane costs the engine: store commits, supervisor requests,
//! Core lock time and the wait for the echo (audit item A3).
//!
//! A person types one byte at a time, and every `pane.input` records its intent durably before
//! the byte reaches the shell (ADR 0117) and has its outcome recorded with the next write
//! (ADR 0147, step R). The test engine runs with `SOYEHT_TEST_MEASURE_COST`, which has it write a
//! log line for every hold of the Core lock and every request to the supervisor, each with the
//! commits the stores have made so far; the test reads the log between two points, so the counts
//! are exact and do not depend on the host. The limits below hold the current numbers, so a
//! change that adds a commit or a round trip to a key fails; a change that removes one lowers
//! them. The echo times depend on the host: they are printed for a meter and bounded loosely.
//!
//! Run it with the release profile for the numbers of the shipped engine:
//! `cargo test --release -p soyeht-engine --features test-hooks --test typing_cost -- --nocapture`
#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use std::{collections::BTreeMap, fs, time::Duration};

mod support;
use support::*;

const MEASURE: (&str, &str) = ("SOYEHT_TEST_MEASURE_COST", "1");
/// Time for the shell's prompt and what the engine does about it to be over.
const SETTLE: Duration = Duration::from_secs(3);
/// Time for the outcome of the last key to be recorded alone (the linger is a quarter second).
const FLUSH: Duration = Duration::from_millis(1_200);
/// The pause after a key's echo of a person who types fast: the keys of a burst.
const BURST_GAP: Duration = Duration::from_millis(20);
/// The pause after a key's echo that is longer than the linger of an outcome: each key is alone.
const ALONE_GAP: Duration = Duration::from_millis(400);
/// The longest the median echo of a key may take; a loaded host does not reach it.
const ECHO_LIMIT: Duration = Duration::from_millis(400);

/// What the engine's log says happened between two points.
#[derive(Default)]
struct Window {
    commits: u64,
    requests: BTreeMap<String, Vec<u64>>,
    /// Microseconds of every hold, by purpose and method.
    holds: BTreeMap<String, Vec<u64>>,
}

impl Window {
    fn request_count(&self) -> usize {
        self.requests.values().map(Vec::len).sum()
    }

    /// The requests the input itself makes: its bytes and the confirmation of the operation. The
    /// reads and acknowledgements of the output that echoes it are the terminal path's.
    fn input_requests(&self) -> usize {
        ["input", "confirm_input"]
            .iter()
            .map(|kind| self.requests.get(*kind).map_or(0, Vec::len))
            .sum()
    }

    fn held_us(&self) -> u64 {
        self.holds.values().flatten().sum()
    }
}

fn percentile(sorted: &[u64], percent: usize) -> u64 {
    sorted
        .get((sorted.len() * percent / 100).min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0)
}

/// The probe lines of `log` from byte `from` to byte `to`, with the commits made before `from`.
fn read_window(log: &std::path::Path, from: usize, to: usize) -> Window {
    let text = fs::read(log).unwrap();
    let text = String::from_utf8_lossy(&text[..to.min(text.len())]).into_owned();
    let (before, inside) = text.split_at(from.min(text.len()));
    let commits_of = |line: &str| -> Option<(Value, u64)> {
        let value: Value = serde_json::from_str(line).ok()?;
        let commits = value["commits"].as_str()?.parse().ok()?;
        Some((value, commits))
    };
    let baseline = before
        .lines()
        .filter_map(commits_of)
        .map(|(_, commits)| commits)
        .next_back()
        .unwrap_or(0);
    let mut window = Window {
        commits: 0,
        ..Window::default()
    };
    let mut last = baseline;
    for (value, commits) in inside.lines().filter_map(commits_of) {
        last = last.max(commits);
        let number = |field: &str| value[field].as_str().and_then(|text| text.parse().ok());
        match value["event"].as_str() {
            Some("test.supervisor_request") => window
                .requests
                .entry(value["kind"].as_str().unwrap().to_owned())
                .or_default()
                .push(number("us").unwrap()),
            Some("test.lock_held") => {
                let method = value["method"].as_str().unwrap();
                let name = if method.is_empty() {
                    value["purpose"].as_str().unwrap().to_owned()
                } else {
                    format!("{}:{method}", value["purpose"].as_str().unwrap())
                };
                window
                    .holds
                    .entry(name)
                    .or_default()
                    .push(number("heldUs").unwrap());
            }
            _ => {}
        }
    }
    window.commits = last - baseline;
    window
}

struct Typed {
    window: Window,
    echoes: Vec<Duration>,
}

fn log_len(rig: &Rig) -> usize {
    fs::metadata(rig.path.join("engine.log")).map_or(0, |meta| usize::try_from(meta.len()).unwrap())
}

/// Types `keys` letters one request each into an idle shell, `gap` after the echo of each, and
/// returns what the log says of that and how long each echo took.
fn type_keys(rig: &Rig, keys: usize, gap: Duration) -> Typed {
    let (mut client, mut watcher, _, pane, pane_ref) = watched_shell(rig);
    wait_for_shell(&mut client, &mut watcher, &pane, pane_ref);
    settle_shell(&mut watcher, SETTLE);
    let start = log_len(rig);
    let mut echoes = Vec::new();
    for key in 0..keys {
        echoes.push(type_letter_and_wait_for_echo(
            &mut client,
            &mut watcher,
            (&pane, pane_ref),
            key,
        ));
        std::thread::sleep(gap);
    }
    std::thread::sleep(FLUSH);
    let end = log_len(rig);
    echoes.sort();
    Typed {
        window: read_window(&rig.path.join("engine.log"), start, end),
        echoes,
    }
}

fn report(name: &str, keys: usize, typed: &Typed) {
    let Typed { window, echoes } = typed;
    let per_key = |total: u64| total as f64 / keys as f64;
    eprintln!("{name}: {keys} keys");
    eprintln!(
        "  store commits: {} ({:.2} per key)",
        window.commits,
        per_key(window.commits)
    );
    eprintln!(
        "  supervisor requests: {} ({:.2} per key), of the input itself {} ({:.2} per key)",
        window.request_count(),
        per_key(window.request_count() as u64),
        window.input_requests(),
        per_key(window.input_requests() as u64)
    );
    for (kind, times) in &window.requests {
        let mut sorted = times.clone();
        sorted.sort_unstable();
        eprintln!(
            "    {kind}: {} ({:.2} per key), round trip p50 {} us, p95 {} us",
            times.len(),
            per_key(times.len() as u64),
            percentile(&sorted, 50),
            percentile(&sorted, 95)
        );
    }
    eprintln!(
        "  Core lock held: {} us per key in {:.2} holds per key",
        per_key(window.held_us()) as u64,
        per_key(window.holds.values().map(Vec::len).sum::<usize>() as u64)
    );
    for (purpose, times) in &window.holds {
        let mut sorted = times.clone();
        sorted.sort_unstable();
        eprintln!(
            "    {purpose}: {} holds ({:.2} per key), {} us per key, p50 {} us, p95 {} us, max {} us",
            times.len(),
            per_key(times.len() as u64),
            per_key(times.iter().sum()) as u64,
            percentile(&sorted, 50),
            percentile(&sorted, 95),
            sorted.last().copied().unwrap_or(0)
        );
    }
    let at = |percent: usize| echoes[(echoes.len() * percent / 100).min(echoes.len() - 1)];
    eprintln!(
        "  echo (probe on): p50 {:?}, p95 {:?}, max {:?}",
        at(50),
        at(95),
        echoes.last().unwrap()
    );
}

/// A pane of a quiet engine makes no commit and no request of its own in a window of the length
/// the keys take, so what the typing tests count is the keys'.
#[test]
fn an_idle_shell_makes_no_commit_and_no_request() {
    let rig = Rig::with_environment(&[MEASURE]);
    let (mut client, mut watcher, _, pane, pane_ref) = watched_shell(&rig);
    wait_for_shell(&mut client, &mut watcher, &pane, pane_ref);
    std::thread::sleep(SETTLE);
    let start = log_len(&rig);
    std::thread::sleep(Duration::from_secs(5));
    let window = read_window(&rig.path.join("engine.log"), start, log_len(&rig));
    eprintln!(
        "idle for 5 s: {} commits, {} supervisor requests {:?}",
        window.commits,
        window.request_count(),
        window.requests.keys().collect::<Vec<_>>()
    );
    assert_eq!(window.commits, 0, "an idle shell made commits");
}

/// Types `keys` letters `gap` apart and holds what they cost to `commits_per_key` commits a key,
/// a tenth more for a loaded host that stalls a key past the linger, and two supervisor requests
/// a key for the input itself: its bytes and the confirmation of the operation before.
fn assert_cost(name: &str, keys: usize, gap: Duration, commits_per_key: usize) {
    let rig = Rig::with_environment(&[MEASURE]);
    let typed = type_keys(&rig, keys, gap);
    report(name, keys, &typed);
    let window = &typed.window;
    assert!(
        window.commits <= (commits_per_key * keys + keys / 10) as u64,
        "{} commits for {keys} keys",
        window.commits
    );
    assert!(
        window.input_requests() <= 2 * keys + 2,
        "{} supervisor requests of the input for {keys} keys",
        window.input_requests()
    );
}

/// Keys that follow each other within the linger of an outcome: the outcome of one is recorded
/// with the intent of the next, so a key costs one commit (measured on main).
#[test]
fn a_burst_of_keys_costs_one_commit_and_two_requests_each() {
    assert_cost("burst", 200, BURST_GAP, 1);
}

/// Keys that are each alone: the outcome waits out its linger and is recorded by itself, so a
/// key costs two commits (measured on main), and the confirmation is sent on its own.
#[test]
fn keys_typed_apart_cost_two_commits_and_two_requests_each() {
    assert_cost("alone", 100, ALONE_GAP, 2);
}

/// The echo of a key with the probe off, as a person feels it.
#[test]
fn the_echo_of_a_typed_burst_stays_prompt() {
    const KEYS: usize = 200;
    let rig = Rig::new();
    let typed = type_keys(&rig, KEYS, BURST_GAP);
    let at = |percent: usize| typed.echoes[(KEYS * percent / 100).min(KEYS - 1)];
    eprintln!(
        "echo of a burst (probe off): p50 {:?}, p95 {:?}, max {:?}",
        at(50),
        at(95),
        typed.echoes.last().unwrap()
    );
    assert!(
        at(50) < ECHO_LIMIT,
        "the median echo of a key took {:?}",
        at(50)
    );
}
