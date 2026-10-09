//! Round-trip latency of the editor's per-keystroke engine request.
//!
//! The suite types a short burst into a small file and checks every revision.
//! To compare branches, measure the full workload in release:
//! `SOYEHT_EDITOR_WRITE_BENCH=1 cargo test --release -p soyeht-engine --test editor_write_latency -- --nocapture`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use std::{
    fs,
    time::{Duration, Instant},
};
use support::Rig;

/// (lines, warm-up writes, measured writes): the suite's burst, then the bench.
const QUICK: (usize, usize, usize) = (2_000, 5, 40);
const BENCH: (usize, usize, usize) = (100_000, 40, 400);

fn percentile(sorted: &[Duration], numerator: usize, denominator: usize) -> Duration {
    sorted[(sorted.len() * numerator / denominator).min(sorted.len() - 1)]
}

#[test]
fn file_write_round_trips_answer_every_keystroke_in_order() {
    let (lines, warmup, measured) = if std::env::var_os("SOYEHT_EDITOR_WRITE_BENCH").is_some() {
        BENCH
    } else {
        QUICK
    };
    let rig = Rig::new();
    let project = rig.path.join("bench-project");
    fs::create_dir_all(project.join("src")).unwrap();
    let text = (0..lines)
        .map(|line| format!("fn line_{line}() {{ let value = {line}; }}\n"))
        .collect::<String>();
    fs::write(project.join("src/large.rs"), text).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Bench","path":project,"idempotencyKey":"bench-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"src/large.rs","idempotencyKey":"bench-open"}),
    );
    let file_id = opened["file"]["fileId"].clone();
    let mut revision = opened["file"]["revision"].as_str().unwrap().to_owned();
    let author = json!({"actor":{"role":"human_local","client":"runtime-test"},"name":"you"});
    let mut samples = Vec::with_capacity(measured);
    for index in 0..warmup + measured {
        let started = Instant::now();
        let written = client.call(
            "file.write",
            json!({
                "fileId":file_id,"baseRevision":revision,"author":author,
                "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":0}},
                    "text":"x"}],
                "idempotencyKey":format!("bench-write-{index}")
            }),
        );
        let elapsed = started.elapsed();
        let next = written["file"]["revision"].as_str().unwrap().to_owned();
        assert_eq!(
            next.parse::<u64>().unwrap(),
            revision.parse::<u64>().unwrap() + 1,
            "every keystroke advances the revision once"
        );
        revision = next;
        if index >= warmup {
            samples.push(elapsed);
        }
    }
    samples.sort();
    eprintln!(
        "editor write latency: lines={lines} samples={} p50={:?} p95={:?} p99={:?} max={:?}",
        samples.len(),
        percentile(&samples, 1, 2),
        percentile(&samples, 95, 100),
        percentile(&samples, 99, 100),
        samples[samples.len() - 1],
    );
}
