//! A scripted session that shows where the Core lock time goes (ADR 0147). Ignored: it takes more
//! than a minute and prints, it asserts nothing. Run it with
//! `cargo test -p soyeht-engine --features soyeht-engine/contract-checks --test lock_profile -- --ignored --nocapture`.
//! It prints the latency of each request kind, the longest silence of a terminal that prints every
//! 20 ms, and the `engine.core_lock_usage` lines the engine wrote for the session.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use support::{Client, Rig, Ticker, longest_gap};

/// How long the engine is given to write its usage lines: they come once a minute.
const USAGE_AFTER: Duration = Duration::from_secs(65);

fn type_into(client: &mut Client, pane: &str, text: &str, key: &str) {
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(text),"idempotencyKey":key}),
    );
}

/// A project of 400 small source files and one large text file.
fn write_project(project: &Path) {
    fs::create_dir_all(project.join("src")).unwrap();
    for n in 0..400 {
        let body = (0..200)
            .map(|line| format!("fn item_{n}_{line}() {{ let foo = {line}; }}\n"))
            .collect::<String>();
        fs::write(project.join("src").join(format!("file{n}.rs")), body).unwrap();
    }
    let large = (0..60_000)
        .map(|n| format!("line {n:06} foo {}\n", "x".repeat(70)))
        .collect::<String>();
    fs::write(project.join("big.txt"), large).unwrap();
}

/// Four terminal panes in one workspace.
fn open_panes(client: &mut Client, project: &Path) -> (String, Vec<String>) {
    let (workspace, first) = client.create_in(project, "p1");
    let mut panes = vec![first];
    for n in 2..=4 {
        let created = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":format!("p{n}")}),
        );
        panes.push(created["paneId"].as_str().unwrap().to_owned());
    }
    thread::sleep(Duration::from_millis(1500));
    (workspace, panes)
}

/// Subscribes a reader to each pane that only drains what the engine sends.
fn watch_panes(rig: &Rig, panes: &[String]) {
    for pane in panes {
        let mut watcher = rig.client();
        watcher.tail(pane);
        thread::spawn(
            move || {
                while let Ok(Some(_)) = Frame::read_from(&mut watcher.socket) {}
            },
        );
    }
}

fn type_slowly(rig: &Rig, pane: String) -> thread::JoinHandle<()> {
    let mut typing = rig.client();
    thread::spawn(move || {
        for n in 0..150 {
            type_into(&mut typing, &pane, "a", &format!("t{n}"));
            thread::sleep(Duration::from_millis(30));
        }
    })
}

type Timings = Vec<(&'static str, Duration)>;

fn timed(timings: &mut Timings, name: &'static str, work: &mut dyn FnMut()) {
    let started = Instant::now();
    work();
    timings.push((name, started.elapsed()));
}

/// One file: open, read a page, edit, and ask the engine its status.
fn work_on_file(mix: &mut Client, workspace: &str, index: usize, timings: &mut Timings) {
    let mut opened = Value::Null;
    let path = format!("src/file{index}.rs");
    timed(timings, "file.open", &mut || {
        opened = mix.open_file(workspace, &path);
    });
    let file = opened["file"].clone();
    let author = json!({"actor":{"role":"human_local","client":"runtime-test"},"name":"you"});
    timed(timings, "file.read", &mut || {
        mix.call(
            "file.read",
            json!({"fileId":file["fileId"],"revision":file["revision"],"fromLine":0,"lineCount":80}),
        );
    });
    timed(timings, "file.write", &mut || {
        mix.call(
            "file.write",
            json!({"fileId":file["fileId"],"baseRevision":file["revision"],"author":author,
                "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":2}},"text":"FN"}],
                "idempotencyKey":format!("w{index}")}),
        );
    });
    timed(timings, "engine.status", &mut || {
        mix.call("engine.status", json!({}));
    });
}

/// The requests of a person working in an editor, from a second connection.
fn request_mix(rig: &Rig, workspace: String, project: &Path) -> thread::JoinHandle<Timings> {
    let mut mix = rig.client();
    let project = project.to_owned();
    thread::spawn(move || {
        let mut timings = Timings::new();
        for round in 0..3 {
            for n in 0..30 {
                work_on_file(&mut mix, &workspace, round * 30 + n, &mut timings);
            }
            let search = json!({"workspaceId":workspace,"query":"foo",
                "caseSensitive":true,"limit":50,"scope":"content","idempotencyKey":format!("s{round}")});
            timed(&mut timings, "big.open", &mut || {
                mix.open_file(&workspace, "big.txt");
            });
            timed(&mut timings, "file.search", &mut || {
                mix.call("file.search", search.clone());
            });
            timed(&mut timings, "state.snapshot", &mut || {
                mix.snapshot();
            });
            timed(&mut timings, "workspace.create", &mut || {
                mix.call(
                    "workspace.create",
                    json!({"name":format!("w{round}"),"path":project,"idempotencyKey":format!("ws{round}")}),
                );
            });
        }
        // The replacement plans and writes under the lock.
        let replace = json!({"workspaceId":workspace,"query":"foo","replacement":"bar",
            "caseSensitive":true,"limit":500,"scope":"content","idempotencyKey":"replace"});
        timed(&mut timings, "file.search+replace", &mut || {
            mix.call("file.search", replace.clone());
        });
        timings
    })
}

fn print_latencies(timings: Timings) {
    let mut by_name: BTreeMap<&str, Vec<Duration>> = BTreeMap::new();
    for (name, took) in timings {
        by_name.entry(name).or_default().push(took);
    }
    for (name, mut durations) in by_name {
        durations.sort();
        eprintln!(
            "REQ {name}: n={} p50={:?} max={:?}",
            durations.len(),
            durations[durations.len() / 2],
            durations[durations.len() - 1]
        );
    }
}

#[test]
#[ignore = "a measurement, not a check"]
fn scripted_session() {
    let rig = Rig::new();
    let project = rig.path.join("project");
    write_project(&project);
    let mut client = rig.client();
    let (workspace, panes) = open_panes(&mut client, &project);
    let ticker = Ticker::start(&rig, &mut client, &panes[0]);
    watch_panes(&rig, &panes[1..]);

    let started = Instant::now();
    type_into(&mut client, &panes[1], "seq 1 400000\n", "output-2");
    type_into(
        &mut client,
        &panes[2],
        "yes 'the quick brown fox jumps over the lazy dog' | head -c 40000000\n",
        "output-3",
    );
    let typist = type_slowly(&rig, panes[3].clone());
    let timings = request_mix(&rig, workspace, &project).join().unwrap();
    typist.join().unwrap();
    thread::sleep(Duration::from_secs(8));

    let ended = Instant::now();
    let arrivals = ticker.finish();
    eprintln!(
        "SESSION ticker frames={} longest_gap={:?} session={:?}",
        arrivals.len(),
        longest_gap(&arrivals, started, ended),
        ended - started
    );
    print_latencies(timings);
    thread::sleep(USAGE_AFTER.saturating_sub(started.elapsed()));
    let log = fs::read_to_string(rig.path.join("engine.log")).unwrap_or_default();
    for line in log
        .lines()
        .filter(|line| line.contains("engine.core_lock_usage"))
    {
        eprintln!("{line}");
    }
}
