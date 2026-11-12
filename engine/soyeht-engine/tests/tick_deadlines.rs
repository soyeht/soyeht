//! The subsystems of the tick work without the tick's safety net (ADR 0147, step 1b).
//!
//! The tick sleeps until the earliest deadline of its subsystems and is raised by their events;
//! its ceiling of one second only catches a subsystem that forgot to say when it needs the tick.
//! These engines run with the ceiling at one minute, with no scan of the local servers (a
//! deadline every five seconds, whose job ends by raising the tick) and with the user's skill
//! folder unfollowed (a probe every two seconds, which every engine has from its start). Each
//! step triggers the work, then waits on the socket without sending a request: a request is a
//! hold of the Core lock, and every hold that is not the tick's runs the tick again, which would
//! hide a missing deadline.
//!
//! Every step has an engine of its own, so that no other subsystem runs the tick for it: an open
//! file does four times a second (the disk watch), a pane every second (the sweep of the
//! supervisor's output), a followed folder of skills every two seconds (the probe). What is left
//! in an engine is the deadline its step is about, and the note that the supervisor link has
//! stayed up (ten seconds, which is why a step waits for six at most): a step whose deadline is
//! missing is done a minute late, not in seconds, and fails. `tick_due`'s unit tests pin the
//! deadline of each family; these show that its work is done by it. The editor's autosave and its
//! disk watch share an engine, since an open file has both, and are proven together. So do the
//! output of a shell and the link: the supervisor is gone for a while, so the engine fails to
//! reach it, and when it is back only the retry deadline of the link runs the tick.
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

/// Past the longest deadline a step relies on (the skills probe of two seconds), short of the
/// supervisor link's ten seconds and of the ceiling.
const WITHIN: Duration = Duration::from_secs(6);

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

/// An engine whose tick has no deadline of its own to fall back on (see the top of the file),
/// with a project of one file.
fn quiet_engine() -> (Rig, std::path::PathBuf, Client, String) {
    let rig = Rig::with_environment(&[
        ("SOYEHT_TEST_TICK_CEILING_MS", "60000"),
        ("SOYEHT_TEST_NO_PORT_SCAN", "1"),
        ("SOYEHT_TEST_NO_USER_SKILLS", "1"),
    ]);
    let project = rig.path.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/lib.rs"), "one\n").unwrap();
    let mut client = rig.subscribed_client();
    let workspace = client.add_workspace(&project, "deadlines");
    client.socket.set_read_timeout(Some(WITHIN)).unwrap();
    (rig, project, client, workspace)
}

#[test]
fn the_editor_saves_and_sees_the_disk_by_its_own_deadlines() {
    let (_rig, project, mut client, workspace) = quiet_engine();

    // The autosave: an edit reaches the disk.
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

    // The disk watch: a change made by another program is reported.
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
}

#[test]
fn a_new_file_in_a_watched_folder_is_announced_when_its_burst_of_events_ends() {
    let (_rig, project, mut client, workspace) = quiet_engine();
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
}

#[test]
fn a_skill_written_by_hand_is_listed_on_the_probe() {
    let (_rig, project, mut client, workspace) = quiet_engine();
    // The rig reads skill folders on the probe.
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
}

#[test]
fn a_shell_is_read_on_a_notice_and_a_supervisor_that_comes_back_is_connected_again() {
    let (mut rig, _project, mut client, workspace) = quiet_engine();
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

    // The link to the supervisor: the pane reads it as lost and the engine tries it again, in vain,
    // for a while. When the supervisor is back only the retry deadline of the link runs the tick,
    // and it is made again, and the pane gets a process, with nobody asking.
    let before = client.pane(&pane)["pid"].clone();
    rig.ptyd.kill().unwrap();
    rig.ptyd.wait().unwrap();
    std::thread::sleep(Duration::from_secs(3));
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
