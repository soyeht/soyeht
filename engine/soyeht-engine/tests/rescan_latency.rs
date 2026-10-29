//! A catalogue rescan asks the person's login shell for its `PATH` again, which takes seconds on
//! a busy machine. Nothing else may wait for that answer: every request needs the Core lock, and
//! a request that holds it while it waits for the shell stalls the whole engine until the
//! client's request timer (`x-clientPolicy`) fires and drops the connection.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::client_policy::REQUEST_TIMEOUT_MS;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

mod support;
use support::*;

/// How long the stand-in login shell takes to answer while a rescan is under way.
const SLOW_SHELL: Duration = Duration::from_secs(4);
/// A request the engine can answer at once. It is far below both the shell and the policy.
const PROMPT_ANSWER: Duration = Duration::from_secs(2);

/// The engine's own record of a Core lock hold long enough to delay every other request.
fn slow_holds(rig: &Rig) -> String {
    fs::read_to_string(rig.path.join("engine.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("engine.core_lock_slow"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn requests_answer_while_a_rescan_waits_for_the_login_shell_during_an_agent_turn() {
    assert!(SLOW_SHELL < Duration::from_millis(REQUEST_TIMEOUT_MS));
    let mut rig = Rig::new();
    let shell_directory = rig.path.join("shell");
    fs::create_dir(&shell_directory).unwrap();
    // The probe takes the last marked `PATH` of its output; the delay is the person's startup files.
    write_script(
        &shell_directory.join("bash"),
        &format!(
            "#!/bin/sh\nif [ -e '{}' ]; then sleep {}; fi\nprintf '\\036%s\\037' \"$PATH\"\n",
            shell_directory.join("slow").display(),
            SLOW_SHELL.as_secs()
        ),
    );
    rig.kill_engine();
    rig.login_shell = Some(shell_directory.join("bash"));
    rig.start();

    let (mut human, pane) = rig.agent();
    let workspace = human.pane(&pane)["workspaceId"].clone();
    human.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"turn-during-rescan"}),
    );
    wait_until("the turn did not start", || {
        human.pane(&pane)["state"] == "working"
    });

    fs::write(shell_directory.join("slow"), b"").unwrap();
    let mut scanner = rig.client();
    let rescan = thread::spawn(move || {
        let started = Instant::now();
        let reply = scanner.call("harness.list", json!({"limit":64,"rescan":true}));
        (reply, started.elapsed())
    });
    // Let the rescan reach the shell, then open a pane: it resolves its harness's executable.
    thread::sleep(Duration::from_millis(400));
    let mut opener = rig.client();
    let opening = thread::spawn(move || {
        opener.call(
            "pane.create",
            json!({"workspaceId":workspace,"mode":"agent","harness":"test-agent",
                "idempotencyKey":"pane-during-rescan"}),
        )
    });
    thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    human.call(
        "pane.draft",
        json!({"paneId":pane,"active":true,"idempotencyKey":"draft-during-rescan"}),
    );
    let answered = started.elapsed();
    assert!(
        answered < PROMPT_ANSWER,
        "pane.draft waited {answered:?} behind a rescan that waits for the login shell; \
         the engine logged: {}",
        slow_holds(&rig)
    );
    opening.join().unwrap();
    let (reply, rescan_took) = rescan.join().unwrap();
    assert!(rescan_took >= SLOW_SHELL - Duration::from_millis(500));
    assert!(
        reply["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}
