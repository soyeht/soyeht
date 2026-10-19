//! What a pane reports while its agent session is open but no turn runs, and
//! what a cancel with nothing to cancel does to the queue.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use std::{
    thread,
    time::{Duration, Instant},
};

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
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while client.pane(&pane)["phrase"] != "Agent turn ended" {
        assert!(Instant::now() < deadline, "the prompt did not run");
        thread::sleep(Duration::from_millis(20));
    }
}
