//! Starting the project tree watcher must not stall the engine (ADR 0079). The system's
//! file-event service can take a second or more to start a stream on a busy machine, and
//! `project.tree.watch` used to start it with the Core lock held, so every request and the
//! terminal output of every pane waited too (`engine.core_lock_slow purpose=dispatch`).
//! The test engine pauses before each stream starts (`SOYEHT_TEST_SLOW_WATCH_MS`).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use std::{
    fs, thread,
    time::{Duration, Instant},
};
use support::{Rig, Ticker, longest_gap};

/// How long starting the stream takes. The old request held the lock for all of it.
const START: &str = "1500";
/// The longest stretch a watcher may get no terminal frame, or a request no reply. With the
/// start under the lock both are as long as the start; without it, a scheduling delay.
const STALL_CEILING: Duration = Duration::from_millis(700);

#[test]
fn starting_a_project_watcher_does_not_stall_terminal_output_or_requests() {
    let rig = Rig::with_environment(&[("SOYEHT_TEST_SLOW_WATCH_MS", START)]);
    let folder = rig.path.join("tree");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("first.txt"), "one\n").unwrap();
    let mut client = rig.client();
    let workspace = client.add_workspace(&folder, "tree");
    let (_, pane) = client.create(&rig);
    let ticker = Ticker::start(&rig, &mut client, &pane);

    let mut watcher = rig.client();
    // Connected before the watch starts: a new connection's greeting waits for the lock too.
    let mut prober = rig.client();
    let started = Instant::now();
    let watching = thread::spawn({
        let workspace = workspace.clone();
        move || {
            let reply = watcher.request(
                "project.tree.watch",
                json!({"workspaceId":workspace,"paths":[""]}),
            );
            (watcher, reply, Instant::now())
        }
    });
    let mut slowest_request = Duration::ZERO;
    while !watching.is_finished() {
        let asked = Instant::now();
        prober.call("engine.status", json!({}));
        slowest_request = slowest_request.max(asked.elapsed());
        thread::sleep(Duration::from_millis(50));
    }
    let (mut watcher, reply, answered) = watching.join().unwrap();
    let ended = Instant::now();
    let arrivals = ticker.finish();
    let silence = longest_gap(&arrivals, started, ended);

    eprintln!(
        "project.tree.watch took {:?}; longest terminal silence {silence:?}; \
         slowest engine.status {slowest_request:?}",
        answered - started
    );
    assert!(reply.get("error").is_none(), "{reply}");
    assert!(
        answered - started >= Duration::from_millis(1_400),
        "the watcher did not start slowly: the test shows nothing"
    );
    assert!(
        silence < STALL_CEILING,
        "no terminal frame for {silence:?}: the watch start held the Core lock"
    );
    assert!(
        slowest_request < STALL_CEILING,
        "a request waited {slowest_request:?}: the watch start held the Core lock"
    );

    // The watch is live: a file created after the reply is announced. The system can deliver
    // file events late, in batches, and sometimes only after a later write, so a new file is
    // created every few seconds until the report arrives, with a deadline far past any load.
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut created = 0;
    let mut next_file = Instant::now();
    loop {
        if Instant::now() >= next_file {
            fs::write(folder.join(format!("later{created}.txt")), "two\n").unwrap();
            created += 1;
            next_file += Duration::from_secs(3);
        }
        // Any request reads the frames that have arrived.
        watcher.call("engine.status", json!({}));
        let told = watcher
            .notifications("project.tree_changed")
            .iter()
            .any(|params| params["workspaceId"] == json!(workspace));
        if told {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "project.tree_changed was not published"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
