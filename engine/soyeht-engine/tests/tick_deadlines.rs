//! The subsystems of the tick work without the tick's safety net (ADR 0147, step 1b).
//!
//! The tick sleeps until the earliest deadline of its subsystems and is raised by their events;
//! its ceiling of one second only catches a subsystem that forgot to say when it needs the tick.
//! This engine runs with the ceiling at one minute. Each step triggers the work, then waits on
//! the socket without sending a request: a request is a hold of the Core lock, and every hold
//! that is not the tick's runs the tick again, which would hide a missing deadline.
//!
//! The steps with no pane come first: a pane brings the sweep of the supervisor's output, a
//! deadline every second that would also run the tick. Other deadlines of the engine run it too
//! (the skills probe at two seconds, the scan of the local servers at five), so a subsystem that
//! forgot its own would here be late by seconds, not by the minute; `tick_due`'s unit tests pin
//! the deadline of each family. What this test shows is that the work of each family (a shell's
//! output, a save, a change on the disk, a new file, a skill, a link that came back) is done
//! within seconds, never left for the minute.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::frame::Frame;
use std::{
    fs,
    time::{Duration, Instant},
};

mod support;
use support::*;

/// Far past the longest deadline a step relies on (the editor autosave, the skills probe of two
/// seconds), far short of the ceiling.
const WITHIN: Duration = Duration::from_secs(12);

/// Waits on the socket, sending nothing, for a notification of one of `methods` that `accept`
/// takes.
fn listen(client: &mut Client, methods: &[&str], accept: impl Fn(&Value) -> bool) -> Option<Value> {
    let deadline = Instant::now() + WITHIN;
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        client
            .socket
            .set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .unwrap();
        let Ok(Some(Frame::Json(bytes))) = Frame::read_from(&mut client.socket) else {
            return None;
        };
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if methods.iter().any(|method| value["method"] == *method) && accept(&value["params"]) {
            return Some(value["params"].clone());
        }
    }
}

fn contains(output: &[u8], text: &[u8]) -> bool {
    output.windows(text.len()).any(|part| part == text)
}

/// Reads the terminal output that has arrived for `pane_ref` for up to `wait`, adding it to `seen`.
fn read_output(client: &mut Client, pane_ref: u64, wait: Duration, seen: &mut Vec<u8>) {
    let deadline = Instant::now() + wait;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        client
            .socket
            .set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .unwrap();
        let Ok(Some(frame)) = Frame::read_from(&mut client.socket) else {
            return;
        };
        if let Frame::Terminal {
            pane_ref: found,
            bytes,
            ..
        } = frame
            && u64::from(found) == pane_ref
        {
            seen.extend(bytes);
        }
    }
}

/// Waits on the socket, sending nothing, for terminal output that contains `text`.
fn listen_for_output(client: &mut Client, pane_ref: u64, text: &str) -> bool {
    let deadline = Instant::now() + WITHIN;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        read_output(client, pane_ref, Duration::from_millis(300), &mut seen);
        if contains(&seen, text.as_bytes()) {
            return true;
        }
    }
    false
}

/// Types into the shell until it answers: a login shell that is still starting drops what is typed
/// before it reads the terminal.
fn wait_for_shell(client: &mut Client, watcher: &mut Client, pane: &str, pane_ref: u64) {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut seen = Vec::new();
    for attempt in 0.. {
        assert!(Instant::now() < deadline, "the shell never answered");
        client.call(
            "pane.input",
            json!({"paneId":pane,"bytes":STANDARD.encode("echo RE\"\"ADY\n"),
                "idempotencyKey":format!("ready-{attempt}")}),
        );
        read_output(watcher, pane_ref, Duration::from_millis(300), &mut seen);
        if contains(&seen, b"READY\r\n") {
            return;
        }
    }
}

/// Waits without a request until `ready` holds.
fn wait_without_asking(ready: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + WITHIN;
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn every_subsystem_of_the_tick_works_with_the_ceiling_at_one_minute() {
    let mut rig = Rig::with_environment(&[("SOYEHT_TEST_TICK_CEILING_MS", "60000")]);
    let project = rig.path.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/lib.rs"), "one\n").unwrap();
    let mut client = rig.subscribed_client();
    let workspace = client.add_workspace(&project, "deadlines");

    // The editor's autosave: an edit reaches the disk by its own deadline.
    let opened = client.open_file(&workspace, "src/lib.rs");
    let file = opened["file"]["fileId"].clone();
    client.call(
        "file.write",
        json!({"fileId":file,"baseRevision":opened["file"]["revision"],
            "author":{"actor":{"role":"human_local","client":"runtime-test"},"name":"you"},
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":0}},
                "text":"autosaved "}],
            "idempotencyKey":"autosave"}),
    );
    assert!(
        wait_without_asking(|| fs::read_to_string(project.join("src/lib.rs"))
            .is_ok_and(|text| text.starts_with("autosaved "))),
        "the editor did not autosave"
    );

    // The editor's disk watch: a change made by another program is reported.
    client.socket.set_read_timeout(Some(WITHIN)).unwrap();
    fs::write(project.join("src/lib.rs"), "changed elsewhere\n").unwrap();
    assert!(
        listen(
            &mut client,
            &["file.changed_on_disk", "file.conflict"],
            |_| true
        )
        .is_some(),
        "the editor did not see a change made on the disk"
    );

    // The project panel: a new file in a watched folder is announced.
    client.call(
        "project.tree.watch",
        json!({"workspaceId":workspace,"paths":[""]}),
    );
    fs::write(project.join("later.txt"), "two\n").unwrap();
    assert!(
        listen(&mut client, &["project.tree_changed"], |params| {
            params["workspaceId"] == json!(workspace)
        })
        .is_some(),
        "the project panel was not told of a new file"
    );

    // The skills: a folder written by hand is listed (the rig reads skill folders on the probe).
    client.call("skills.list", json!({"workspaceId":workspace}));
    let skill = project.join(".agents/skills/by-hand");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: by-hand\ndescription: Written by hand.\n---\nDo it.\n",
    )
    .unwrap();
    assert!(
        listen(&mut client, &["skills.changed"], |params| {
            params["workspaceId"] == json!(workspace)
        })
        .is_some(),
        "the skills were not read again"
    );

    // The terminal path: a shell prints what it is told, read when the supervisor says it has
    // output.
    let mut watcher = rig.client();
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":"shell"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("the shell started", || !client.pane(&pane)["pid"].is_null());
    let (info, _) = watcher.tail(&pane);
    let pane_ref = info["binaryPaneRef"].as_u64().unwrap();
    // A login shell that is still starting drops what is typed before it reads the terminal.
    wait_for_shell(&mut client, &mut watcher, &pane, pane_ref);
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode("echo DEAD\"\"LINE\n"),
            "idempotencyKey":"deadline"}),
    );
    assert!(
        listen_for_output(&mut watcher, pane_ref, "DEADLINE\r\n"),
        "the output of the shell was not read"
    );

    // The link to the supervisor: it is made again, and the pane gets a process, with nobody
    // asking.
    let before = client.pane(&pane)["pid"].clone();
    rig.restart_ptyd();
    assert!(
        listen(&mut client, &["pane.updated"], |params| {
            params["pane"]["paneId"] == json!(pane)
                && !params["pane"]["pid"].is_null()
                && params["pane"]["pid"] != before
        })
        .is_some(),
        "the pane was not given a process after the supervisor came back"
    );
}
