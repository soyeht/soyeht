//! An agent that asks every process for the protocol sign-in (Cursor): the sign-in the person
//! chose once authenticates every process the engine starts later for that harness, so a pane
//! opens its session, a second pane needs no press, and a switch into the harness works
//! (ADR 0100).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::{fs, path::PathBuf};

mod support;
use support::*;

const METHOD: &str = "existing-login";

/// An engine with a harness that wants `authenticate` from every process and an ordinary one to
/// switch from. Returns the native state directory of the first.
fn rig() -> (Rig, PathBuf) {
    let rig = Rig::new();
    let native = rig.path.join("native");
    fs::create_dir_all(&native).unwrap();
    let binary = sibling_binary("soyeht-fake-acp");
    rig.write_harnesses(&json!([
        {"id":"session-auth","name":"Session auth","command":binary,
            "args":["--state-dir", native.to_string_lossy(), "--require-login", "--session-auth"],
            "version":"test"},
        {"id":"plain","name":"Plain","command":binary,
            "args":["--state-dir", rig.path.join("plain-native").to_string_lossy()],
            "version":"test"}
    ]));
    (rig, native)
}

fn create_pane(client: &mut Client, workspace: &str, harness: &str, key: &str) -> String {
    client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"agent","harness":harness,"idempotencyKey":key}),
    )["paneId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn wait_for_sign_in_request(client: &mut Client, pane: &str) {
    wait_until("agent requests sign-in", || {
        client.pane(pane)["phrase"] == "Sign-in required"
    });
    assert_eq!(client.pane(pane)["agent"]["authMethods"][0]["id"], METHOD);
}

fn wait_for_session(client: &mut Client, pane: &str) {
    wait_until("session opens in a new agent process", || {
        client.pane(pane)["agent"]["sessionId"].is_string()
    });
}

/// The person signs in outside the engine, then presses the protocol sign-in in `pane`.
fn press_sign_in(client: &mut Client, native: &std::path::Path, pane: &str) {
    wait_for_sign_in_request(client, pane);
    fs::write(native.join("login.ok"), "account-a").unwrap();
    client.call(
        "pane.auth.start",
        json!({"paneId":pane,"authMethodId":METHOD,"idempotencyKey":format!("press-{pane}")}),
    );
}

/// A pane of the ordinary harness with its session open.
fn plain_pane(client: &mut Client, workspace: &str) -> String {
    let pane = create_pane(client, workspace, "plain", "plain-pane");
    wait_for_session(client, &pane);
    pane
}

/// The signed-in harness's client and workspace, and a pane of the ordinary harness to switch.
fn signed_in_with_plain_pane(rig: &Rig, native: &std::path::Path) -> (Client, String, String) {
    let (mut client, workspace, _) = signed_in_pane(rig, native);
    let other = plain_pane(&mut client, &workspace);
    (client, workspace, other)
}

/// Switches the pane into the harness that wants `authenticate`; returns the whole response.
fn switch_into_session_auth(client: &mut Client, pane: &str) -> Value {
    client.request(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"session-auth","idempotencyKey":"switch-in"}),
    )
}

/// A client with a workspace and a pane of the harness the person has signed in, as far as
/// pressing its protocol sign-in and seeing the session open.
fn signed_in_pane(rig: &Rig, native: &std::path::Path) -> (Client, String, String) {
    let mut client = rig.client();
    let (workspace, _) = client.create(rig);
    let pane = create_pane(&mut client, &workspace, "session-auth", "first-pane");
    press_sign_in(&mut client, native, &pane);
    // The engine restarts the agent after the capability probe, and the restarted
    // process asks to be authenticated again before it opens the pane's session.
    wait_for_session(&mut client, &pane);
    (client, workspace, pane)
}

#[test]
fn the_chosen_protocol_sign_in_authenticates_each_new_agent_process() {
    let (rig, native) = rig();
    signed_in_pane(&rig, &native);
}

#[test]
fn a_second_pane_of_the_harness_needs_no_press() {
    let (rig, native) = rig();
    let (mut client, workspace, _) = signed_in_pane(&rig, &native);
    let second = create_pane(&mut client, &workspace, "session-auth", "second-pane");
    wait_for_session(&mut client, &second);
    assert_ne!(client.pane(&second)["phrase"], "Sign-in required");
}

#[test]
fn the_choice_survives_an_engine_restart_for_a_pane_created_after_it() {
    let (mut rig, native) = rig();
    let (client, workspace, _) = signed_in_pane(&rig, &native);
    drop(client);
    rig.kill_engine();
    rig.start();

    let mut client = rig.client();
    let second = create_pane(
        &mut client,
        &workspace,
        "session-auth",
        "after-restart-pane",
    );
    wait_for_session(&mut client, &second);
}

#[test]
fn a_switch_into_the_harness_authenticates_its_candidate() {
    let (rig, native) = rig();
    let (mut client, _, other) = signed_in_with_plain_pane(&rig, &native);
    let switched = switch_into_session_auth(&mut client, &other);
    assert_eq!(switched["result"]["toHarness"], "session-auth");
    assert_eq!(client.pane(&other)["agent"]["harness"], "session-auth");
    assert!(client.pane(&other)["agent"]["sessionId"].is_string());
}

#[test]
fn a_switch_into_the_harness_fails_while_no_sign_in_was_chosen() {
    let (rig, native) = rig();
    fs::write(native.join("login.ok"), "account-a").unwrap();
    let mut client = rig.client();
    let (workspace, _) = client.create(&rig);
    let other = plain_pane(&mut client, &workspace);
    let failed = switch_into_session_auth(&mut client, &other);
    assert_eq!(failed["error"]["data"]["kind"], "harness_switch_failed");
    assert_eq!(client.pane(&other)["agent"]["harness"], "plain");
}

#[test]
fn a_refused_sign_in_is_forgotten_for_the_whole_harness() {
    let (rig, native) = rig();
    let (mut client, workspace, _) = signed_in_pane(&rig, &native);

    // The account is gone: the next process is refused and asks for the sign-in again.
    fs::remove_file(native.join("login.ok")).unwrap();
    let second = create_pane(&mut client, &workspace, "session-auth", "second-pane");
    wait_for_sign_in_request(&mut client, &second);

    // The account is back, but nobody pressed the sign-in again: a third pane still asks.
    fs::write(native.join("login.ok"), "account-b").unwrap();
    let third = create_pane(&mut client, &workspace, "session-auth", "third-pane");
    wait_for_sign_in_request(&mut client, &third);
}

#[test]
fn a_switch_candidate_the_agent_refuses_forgets_the_harness_sign_in() {
    let (rig, native) = rig();
    let (mut client, workspace, other) = signed_in_with_plain_pane(&rig, &native);

    // The account is gone: the candidate's `authenticate` is refused and the switch fails.
    fs::remove_file(native.join("login.ok")).unwrap();
    let failed = switch_into_session_auth(&mut client, &other);
    assert_eq!(failed["error"]["data"]["kind"], "harness_switch_failed");
    assert!(
        failed["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("requires sign-in")),
        "{failed}"
    );
    assert_eq!(client.pane(&other)["agent"]["harness"], "plain");

    // The account is back, but nobody pressed the sign-in again: a new pane still asks.
    fs::write(native.join("login.ok"), "account-b").unwrap();
    let next = create_pane(&mut client, &workspace, "session-auth", "next-pane");
    wait_for_sign_in_request(&mut client, &next);
}
