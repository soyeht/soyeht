//! Agents of two different harnesses talking to each other through the Soyeht
//! MCP server each of them was given: the sender calls `message_agent`, the
//! recipient receives a peer prompt with the reply route and answers with
//! `message_agent`, and both transcripts keep the attributed messages.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::{thread, time::Duration, time::Instant};

mod support;
use support::*;

const CODEX: &str = "Test Codex";
const CLAUDE: &str = "Test Claude";

struct Pair {
    rig: Rig,
    human: Client,
    codex: String,
    claude: String,
}

impl Pair {
    fn new() -> Self {
        let rig = Rig::new();
        let fake = sibling_binary("soyeht-fake-acp");
        rig.write_harnesses(&json!([
            {"id":"test-codex","name":CODEX,"command":fake,
                "args":["--state-dir",rig.path.join("native-codex")],"version":"test"},
            {"id":"test-claude","name":CLAUDE,"command":fake,
                "args":["--state-dir",rig.path.join("native-claude")],"version":"test"},
        ]));
        let mut human = rig.client();
        let workspace = human.call(
            "workspace.create",
            json!({"name":"Peers","path":rig.path,"idempotencyKey":"peers-workspace"}),
        )["workspace"]["workspaceId"]
            .clone();
        let (human, codex) = rig.open_harness_agent(human, &workspace, "test-codex");
        let (human, claude) = rig.open_harness_agent(human, &workspace, "test-claude");
        Self {
            rig,
            human,
            codex,
            claude,
        }
    }

    fn prompt(&mut self, pane: &str, text: &str) {
        self.human.call(
            "pane.prompt",
            json!({"paneId":pane,"text":text,"idempotencyKey":format!("prompt-{pane}-{text}")}),
        );
    }

    /// The first transcript message of `pane` that `accept` takes.
    fn wait_for(&mut self, pane: &str, what: &str, accept: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
        loop {
            let items = transcript_items(&mut self.human, pane);
            if let Some(found) = items.iter().find(|item| accept(item)) {
                return found.clone();
            }
            assert!(
                Instant::now() < deadline,
                "{what} was not recorded: {items:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// A peer message `text` from `sender` (named `harness`) in `recipient`'s transcript.
    fn wait_for_peer_message(&mut self, recipient: &str, sender: &str, harness: &str, text: &str) {
        let message = self.wait_for(recipient, text, |item| {
            item["role"] == "user" && item["text"] == text
        });
        assert_eq!(message["agentMessage"]["paneId"], sender);
        assert_eq!(message["agentMessage"]["harness"], harness);
        assert_eq!(message["agentMessage"]["name"], harness);
    }

    fn wait_for_agent_text(&mut self, pane: &str, text: &str) {
        self.wait_for(pane, text, |item| {
            item["role"] == "assistant" && item["kind"] == "message" && item["text"] == text
        });
    }
}

#[test]
fn a_codex_pane_asks_a_claude_pane_and_receives_its_answer_and_back() {
    let mut pair = Pair::new();
    let (codex, claude) = (pair.codex.clone(), pair.claude.clone());

    pair.prompt(&codex, "list-peers");
    pair.wait_for_agent_text(&codex, &format!("Peers: {CLAUDE}"));
    pair.prompt(&claude, "list-peers");
    pair.wait_for_agent_text(&claude, &format!("Peers: {CODEX}"));

    pair.prompt(
        &codex,
        &format!("peer-send:{claude}:ask: capital of France?"),
    );
    pair.wait_for_peer_message(&claude, &codex, CODEX, "ask: capital of France?");
    pair.wait_for_agent_text(&claude, "Answered Test Codex: capital of France?");
    pair.wait_for_peer_message(&codex, &claude, CLAUDE, "answer: capital of France?");
    pair.wait_for_agent_text(
        &codex,
        "Received from Test Claude: answer: capital of France?",
    );

    pair.prompt(&claude, &format!("peer-send:{codex}:ask: and Spain?"));
    pair.wait_for_peer_message(&codex, &claude, CLAUDE, "ask: and Spain?");
    pair.wait_for_peer_message(&claude, &codex, CODEX, "answer: and Spain?");

    // The harness received the reply route; the transcript kept only the words.
    let session = pair.human.pane(&claude)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let native: Value = serde_json::from_slice(
        &std::fs::read(
            pair.rig
                .path
                .join("native-claude")
                .join(format!("{session}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let route = format!("targetPaneId={codex}.\n\nask: capital of France?");
    assert!(
        native["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|turn| turn[0].as_str().unwrap().ends_with(&route))
    );
}
