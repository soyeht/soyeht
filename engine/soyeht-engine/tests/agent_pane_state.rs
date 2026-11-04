//! What a pane reports while its agent session is open but no turn runs, and
//! what a cancel with nothing to cancel does to the queue.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use std::{thread, time::Duration};

mod support;
use support::*;

#[test]
fn a_pane_with_an_open_session_is_idle_before_its_first_prompt() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let view = client.pane(&pane);
    assert_eq!(view["state"], "idle", "{view}");
    assert_eq!(view["phrase"], "Agent session ready");
    let listed = client
        .section("panes")
        .into_iter()
        .find(|item| item["paneId"] == pane.as_str())
        .unwrap();
    assert_eq!(listed["state"], "idle", "{listed}");
}

#[test]
fn cancelling_an_idle_pane_does_not_pause_its_queue() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let cancelled = client.call(
        "pane.cancel",
        json!({"paneId":pane,"idempotencyKey":"cancel-idle"}),
    );
    assert_eq!(cancelled["cancelling"], false, "{cancelled}");
    assert_eq!(
        client.call("pane.queue.list", json!({"paneId":pane}))["paused"],
        false
    );
    assert_eq!(client.pane(&pane)["agent"]["queuePaused"], false);

    // The queue still runs the next prompt without an explicit resume.
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"after-idle-cancel"}),
    );
    wait_until("the prompt did not run", || {
        client.pane(&pane)["phrase"] == "Agent turn ended"
    });
}

/// An adapter can announce its commands before the answer to `session/new` and after the
/// answer to the probe's `session/set_config_option`. The pane has no session identity to match
/// such an update against, and the update is no reason to stop the agent.
#[test]
fn an_update_before_the_session_identity_does_not_stop_the_pane() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent_with(&["--updates-around-results"]);
    let view = client.pane(&pane);
    assert_eq!(view["state"], "idle", "{view}");
    // The first run of an agent version restarts the process after its probe; an update that
    // arrives between the probe's end and the restart has no session to belong to either.
    thread::sleep(Duration::from_millis(500));
    let view = client.pane(&pane);
    assert_eq!(view["state"], "idle", "{view}");
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"after-early-update"}),
    );
    wait_until("the prompt did not run", || {
        client.pane(&pane)["phrase"] == "Agent turn ended"
    });
}
