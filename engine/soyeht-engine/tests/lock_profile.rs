//! A scripted session that shows where the Core lock time goes (ADR 0147). Ignored: it takes a
//! minute and prints, it asserts nothing. Run it with
//! `cargo test -p soyeht-engine --features soyeht-engine/contract-checks --test lock_profile -- --ignored --nocapture`.
//! It prints the latency of each request kind, the longest silence of a terminal that prints every
//! 20 ms, and the `engine.core_lock_usage` lines the engine wrote for the session.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    fs, thread,
    time::{Duration, Instant},
};
use support::{Rig, Ticker, longest_gap};

fn input(client: &mut support::Client, pane: &str, text: &str, key: &str) {
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(text),"idempotencyKey":key}),
    );
}

#[test]
#[ignore = "a measurement, not a check"]
fn scripted_session() {
    let rig = Rig::new();
    let project = rig.path.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    for n in 0..400 {
        let body = (0..200)
            .map(|l| format!("fn item_{n}_{l}() {{ let foo = {l}; }}\n"))
            .collect::<String>();
        fs::write(project.join("src").join(format!("file{n}.rs")), body).unwrap();
    }
    let big = (0..60_000)
        .map(|n| format!("line {n:06} foo {}\n", "x".repeat(70)))
        .collect::<String>();
    fs::write(project.join("big.txt"), big).unwrap();

    let mut client = rig.client();
    let (workspace, pane1) = client.create_in(&project, "p1");
    let mut panes = vec![pane1.clone()];
    for i in 2..=4 {
        let pane = client.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":format!("p{i}")}),
        )["paneId"]
            .as_str()
            .unwrap()
            .to_owned();
        panes.push(pane);
    }
    thread::sleep(Duration::from_millis(1500));
    // Subscribers for every pane (replay + live).
    let ticker = Ticker::start(&rig, &mut client, &pane1);
    let mut watchers = Vec::new();
    for pane in &panes[1..] {
        let mut watcher = rig.client();
        watcher.tail(pane);
        let handle = thread::spawn(move || {
            // drain frames until the socket closes
            while let Ok(Some(_)) = soyeht_protocol::frame::Frame::read_from(&mut watcher.socket) {}
        });
        watchers.push(handle);
    }
    let started = Instant::now();
    input(&mut client, &panes[1], "seq 1 400000\n", "o2");
    input(
        &mut client,
        &panes[2],
        "yes 'the quick brown fox jumps over the lazy dog' | head -c 40000000\n",
        "o3",
    );

    // Typing in pane 4.
    let typist = {
        let mut typing = rig.client();
        let pane = panes[3].clone();
        thread::spawn(move || {
            for n in 0..150 {
                input(&mut typing, &pane, "a", &format!("t{n}"));
                thread::sleep(Duration::from_millis(30));
            }
        })
    };
    // Request mix.
    let mixer = {
        let mut mix = rig.client();
        let workspace = workspace.clone();
        thread::spawn(move || {
            let mut latencies: Vec<(String, Duration)> = Vec::new();
            let mut timed = |name: &str, f: &mut dyn FnMut()| {
                let t = Instant::now();
                f();
                latencies.push((name.to_owned(), t.elapsed()));
            };
            for round in 0..3 {
                for n in 0..30 {
                    let path = format!("src/file{}.rs", round * 30 + n);
                    let mut opened = Value::Null;
                    timed("file.open", &mut || {
                        opened = mix.open_file(&workspace, &path)
                    });
                    let file = opened["file"].clone();
                    timed("file.read", &mut || {
                        mix.call("file.read", json!({"fileId":file["fileId"],"revision":file["revision"],"fromLine":0,"lineCount":80}));
                    });
                    timed("file.write", &mut || {
                        mix.call("file.write", json!({"fileId":file["fileId"],"baseRevision":file["revision"],"author":{"actor":{"role":"human_local","client":"runtime-test"},"name":"you"},"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":2}},"text":"FN"}],"idempotencyKey":format!("w{round}-{n}")}));
                    });
                    timed("engine.status", &mut || {
                        mix.call("engine.status", json!({}));
                    });
                }
                timed("big.open", &mut || {
                    mix.open_file(&workspace, "big.txt");
                });
                timed("file.search", &mut || {
                    mix.call("file.search", json!({"workspaceId":workspace,"query":"foo","caseSensitive":true,"limit":50,"scope":"content","idempotencyKey":format!("s{round}")}));
                });
                timed("state.snapshot", &mut || {
                    mix.snapshot();
                });
                timed("workspace.create", &mut || {
                    mix.call("workspace.create", json!({"name":format!("w{round}"),"path":project,"idempotencyKey":format!("ws{round}")}));
                });
            }
            // search with replacement (plans and writes under the lock)
            timed("file.search+replace", &mut || {
                mix.call("file.search", json!({"workspaceId":workspace,"query":"foo","replacement":"bar","caseSensitive":true,"limit":500,"scope":"content","idempotencyKey":"replace-1"}));
            });
            latencies
        })
    };
    let latencies = mixer.join().unwrap();
    typist.join().unwrap();
    thread::sleep(Duration::from_secs(8));
    let ended = Instant::now();
    let arrivals = ticker.finish();
    let gap = longest_gap(&arrivals, started, ended);
    eprintln!(
        "SESSION ticker frames={} longest_gap={gap:?} session={:?}",
        arrivals.len(),
        ended - started
    );
    let mut by: std::collections::BTreeMap<String, Vec<Duration>> = Default::default();
    for (n, d) in latencies {
        by.entry(n).or_default().push(d);
    }
    for (n, mut v) in by {
        v.sort();
        eprintln!(
            "REQ {n}: n={} p50={:?} max={:?}",
            v.len(),
            v[v.len() / 2],
            v[v.len() - 1]
        );
    }
    drop(watchers);
    // The engine writes its usage lines once a minute.
    thread::sleep(Duration::from_secs(65).saturating_sub(started.elapsed()));
    let log = fs::read_to_string(rig.path.join("engine.log")).unwrap_or_default();
    for line in log
        .lines()
        .filter(|line| line.contains("engine.core_lock_usage"))
    {
        eprintln!("{line}");
    }
}
