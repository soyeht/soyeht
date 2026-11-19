//! More panes than the 256 descriptors a service starts with: the supervisor and the engine raise
//! their own limit, so every terminal starts and answers. A supervisor that kept the limit it
//! started with fails to start the pane after about seventy.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

mod support;
use support::*;

/// The limit of a service that the system started on macOS.
const STARTED_WITH: u64 = 256;
const PANES: usize = 300;

#[test]
fn three_hundred_terminals_all_start_and_answer_with_a_service_limit_of_256() {
    // The programs the rig starts inherit the limit of a service that the system started.
    soyeht_platform::testing::lower_open_files_limit(STARTED_WITH).unwrap();
    // The plainest shell there is: the test is about descriptors, not about a person's startup files.
    let rig = Rig::with_environment(&[("SHELL", "/bin/sh")]);
    let mut client = rig.client();
    let workspace = client.add_workspace(&rig.path, "many-panes");
    let requests: Vec<u64> = (0..PANES)
        .map(|n| {
            client.send(
                "pane.create",
                json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":format!("pane-{n}")}),
            )
        })
        .collect();
    let panes: Vec<String> = requests
        .into_iter()
        .map(|id| {
            let reply = client.wait(id);
            assert!(reply["error"].is_null(), "{reply}");
            reply["result"]["paneId"].as_str().unwrap().to_owned()
        })
        .collect();

    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT * 2;
    loop {
        let all = client.section("panes");
        let started = all
            .iter()
            .filter(|pane| panes.iter().any(|id| pane["paneId"] == *id) && !pane["pid"].is_null())
            .count();
        if started == PANES {
            break;
        }
        let dead = all.iter().filter(|pane| pane["state"] == "dead").count();
        assert!(
            Instant::now() < deadline && dead == 0,
            "{started} of {PANES} terminals started and {dead} ended dead"
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    // Every shell reads what it is typed: it leaves a file named for its pane.
    for (n, pane) in panes.iter().enumerate() {
        client.call(
            "pane.input",
            json!({"paneId":pane,"bytes":STANDARD.encode(format!("touch answered-{n}\n")),"idempotencyKey":format!("input-{n}")}),
        );
    }
    wait_until("every terminal answers", || {
        (0..PANES).all(|n| rig.path.join(format!("answered-{n}")).exists())
    });
    let _: Value = client.call("engine.status", json!({}));
}
