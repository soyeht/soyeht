//! An agent that asks every process for the protocol sign-in (Cursor): the sign-in the person
//! chose once also authenticates the processes the engine starts later, so the pane opens its
//! session instead of asking again.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use std::fs;

mod support;
use support::*;

const METHOD: &str = "existing-login";

#[test]
fn the_chosen_protocol_sign_in_authenticates_each_new_agent_process() {
    let rig = Rig::new();
    let native = rig.path.join("native");
    fs::create_dir_all(&native).unwrap();
    fs::write(
        rig.path.join("harnesses.json"),
        serde_json::to_vec(&json!([{
            "id":"session-auth","name":"Session auth","command":sibling_binary("soyeht-fake-acp"),
            "args":["--state-dir", native.to_string_lossy(), "--require-login", "--session-auth"],
            "version":"test"
        }]))
        .unwrap(),
    )
    .unwrap();
    let mut client = rig.client();
    let (workspace, _) = client.create(&rig);
    let pane = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":"session-auth",
            "idempotencyKey":"session-auth-pane"}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_until("agent requests sign-in", || {
        client.pane(&pane)["phrase"] == "Sign-in required"
    });
    let methods = client.pane(&pane)["agent"]["authMethods"].clone();
    assert_eq!(methods[0]["id"], METHOD);

    // The person signs in outside the engine, then confirms it in the pane.
    fs::write(native.join("login.ok"), "account-a").unwrap();
    client.call(
        "pane.auth.start",
        json!({"paneId":pane,"authMethodId":METHOD,"idempotencyKey":"session-auth-start"}),
    );
    // The engine restarts the agent after the capability probe, and the restarted
    // process asks to be authenticated again before it opens the pane's session.
    wait_until("session opens in a new agent process", || {
        client.pane(&pane)["agent"]["sessionId"].is_string()
    });
}
