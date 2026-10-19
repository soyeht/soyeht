//! A pane that waits for sign-in records the person's prompts, explains the
//! wait, and recovers when the sign-in is repaired outside Soyeht, without
//! being closed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::fs;

mod support;
use support::*;

const SIGNED_OUT: &str = "Sign-in required";

/// A pane of a harness that refuses to open a session until `native/login.ok`
/// exists, and one that is signed out to begin with.
fn signed_out_pane() -> (Rig, Client, String) {
    pane_of_a_login_harness(false)
}

fn pane_of_a_login_harness(signed_in: bool) -> (Rig, Client, String) {
    let rig = Rig::new();
    let native = rig.path.join("native");
    fs::create_dir_all(&native).unwrap();
    if signed_in {
        sign_in_outside_soyeht(&rig);
    }
    rig.write_harnesses(&json!([{
        "id":"auth-test","name":"Auth test","command":sibling_binary("soyeht-fake-acp"),
        "args":["--state-dir",native,"--require-login"],"version":"test"
    }]));
    let mut client = rig.client();
    let (workspace, _) = client.create(&rig);
    let pane = create_agent_pane(&mut client, &json!(workspace), "auth-test");
    wait_until("the agent opens a session or asks for sign-in", || {
        let view = client.pane(&pane);
        view["phrase"] == SIGNED_OUT || view["agent"]["sessionId"].is_string()
    });
    (rig, client, pane)
}

/// What the person does in their own terminal.
fn sign_in_outside_soyeht(rig: &Rig) {
    fs::write(rig.path.join("native/login.ok"), "account-a").unwrap();
}

fn send(client: &mut Client, pane: &str, text: &str) {
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":text,"idempotencyKey":format!("send-{text}")}),
    );
}

fn texts(items: &[Value], role: &str) -> Vec<String> {
    items
        .iter()
        .filter(|item| item["role"] == role && item["kind"] != "thought")
        .map(|item| item["text"].as_str().unwrap().to_owned())
        .collect()
}

/// Waits until `count` replies of the agent contain `word`.
fn wait_for_answers(client: &mut Client, pane: &str, word: &str, count: usize) {
    wait_until("the prompts were answered", || {
        let items = transcript_items(client, pane);
        texts(&items, "assistant")
            .iter()
            .filter(|text| text.contains(word))
            .count()
            == count
    });
}

fn queued_count(client: &mut Client, pane: &str) -> usize {
    client.pane(pane)["agent"]["queue"]
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn a_prompt_sent_while_signed_out_is_recorded_and_the_pane_says_why_it_waits() {
    let (rig, mut client, pane) = signed_out_pane();
    send(&mut client, &pane, "first question");
    wait_until("the prompt and its notice are recorded", || {
        texts(&transcript_items(&mut client, &pane), "system")
            .iter()
            .any(|text| text.contains("Sign-in is required"))
    });
    let items = transcript_items(&mut client, &pane);
    assert_eq!(texts(&items, "user"), ["first question"]);
    assert_eq!(client.pane(&pane)["phrase"], SIGNED_OUT);
    assert_eq!(queued_count(&mut client, &pane), 1);

    // A second prompt checks again, is recorded once, and still waits.
    send(&mut client, &pane, "second question");
    wait_until("the second prompt is recorded", || {
        texts(&transcript_items(&mut client, &pane), "user").len() == 2
    });
    assert_eq!(queued_count(&mut client, &pane), 2);
    drop(rig);
}

#[test]
fn queued_prompts_run_in_order_once_the_sign_in_is_repaired_outside_soyeht() {
    let (rig, mut client, pane) = signed_out_pane();
    send(&mut client, &pane, "first question");
    wait_until("the first prompt is recorded", || {
        texts(&transcript_items(&mut client, &pane), "user").len() == 1
    });

    sign_in_outside_soyeht(&rig);
    send(&mut client, &pane, "second question");
    wait_for_answers(&mut client, &pane, "question", 2);
    wait_until("the queue is empty", || {
        queued_count(&mut client, &pane) == 0
    });

    let items = transcript_items(&mut client, &pane);
    // Each prompt is in the transcript once, however it reached the agent.
    assert_eq!(texts(&items, "user"), ["first question", "second question"]);
    let view = client.pane(&pane);
    assert!(view["agent"]["sessionId"].is_string(), "{view}");
    assert_ne!(view["phrase"], SIGNED_OUT);
}

#[test]
fn a_turn_rejected_for_authentication_recovers_on_the_next_prompt() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    send(&mut client, &pane, "auth-expired");
    wait_until("the pane asks for sign-in", || {
        client.pane(&pane)["phrase"] == SIGNED_OUT
    });
    assert_ne!(client.pane(&pane)["state"], "dead");

    // The harness account works again; the next prompt checks and runs.
    send(&mut client, &pane, "hello again");
    wait_until("the prompt is answered", || {
        texts(&transcript_items(&mut client, &pane), "assistant")
            .iter()
            .any(|text| text.contains("hello again"))
    });
    // The reopened session replays what the agent kept, so only the new
    // prompt is certain to be there.
    let items = transcript_items(&mut client, &pane);
    assert_eq!(texts(&items, "user").last().unwrap(), "hello again");
    assert_ne!(client.pane(&pane)["phrase"], SIGNED_OUT);
}

#[test]
fn a_prompt_recorded_while_signed_out_survives_the_session_reload() {
    let (rig, mut client, pane) = pane_of_a_login_harness(true);
    send(&mut client, &pane, "first turn");
    wait_until("the first turn is answered", || {
        !texts(&transcript_items(&mut client, &pane), "assistant").is_empty()
    });
    // The account stops working during a turn; the agent keeps its session.
    send(&mut client, &pane, "auth-expired");
    wait_until("the pane asks for sign-in", || {
        client.pane(&pane)["phrase"] == SIGNED_OUT
    });
    fs::remove_file(rig.path.join("native/login.ok")).unwrap();
    send(&mut client, &pane, "waiting prompt");
    wait_until("the waiting prompt is recorded", || {
        texts(&transcript_items(&mut client, &pane), "system")
            .iter()
            .any(|text| text.contains("Sign-in is required"))
    });

    sign_in_outside_soyeht(&rig);
    send(&mut client, &pane, "repaired prompt");
    wait_for_answers(&mut client, &pane, "prompt", 2);
    wait_until("the queue is empty", || {
        queued_count(&mut client, &pane) == 0
    });

    // The reloaded session replaced the transcript; the waiting prompt is in it once.
    let users = texts(&transcript_items(&mut client, &pane), "user");
    assert_eq!(users.first().unwrap(), "first turn", "{users:?}");
    assert_eq!(
        users[users.len() - 2..],
        ["waiting prompt", "repaired prompt"],
        "{users:?}"
    );
    assert_eq!(
        users
            .iter()
            .filter(|text| *text == "waiting prompt")
            .count(),
        1
    );
}
