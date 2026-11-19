//! A `pane.update` that a person's request makes is published with the actor of that request
//! (the store adds it to every event of a mutation), so each of the nine updates declares it.
//! With `contract-checks` on, an update without the field fails the whole request after it
//! committed, and none of its events is sent.

use super::{
    Changes,
    state::{Phase, State},
    subagents::{self, Scope, Spawn},
    transcript::{self, TextChunk},
};
use crate::{core::Core, outbox, test_panes};
use serde_json::{Value, json};
use soyeht_acp::subagents::Spawned;

const PANE: &str = "agent-pane";

struct Rig {
    core: Core,
    directory: std::path::PathBuf,
    connection: String,
    _receiver: outbox::Receiver,
}

impl Rig {
    fn new() -> Self {
        let (mut core, directory) = test_panes::core_with_workspace("workspace");
        test_panes::insert_agent_pane(&mut core, &directory, PANE, "workspace");
        let (connection, receiver) = test_panes::handshaken_connection(&mut core);
        Self {
            core,
            directory,
            connection,
            _receiver: receiver,
        }
    }

    /// Commits what the engine itself recorded (no person's request): a state the pane was in
    /// before the person acted.
    fn stage(&mut self, build: impl FnOnce(&Core, &mut State, &mut Changes)) {
        let mut state = self.core.agent_state(PANE).unwrap();
        let mut changes = Changes::default();
        build(&self.core, &mut state, &mut changes);
        let saved = self.core.pane(PANE).unwrap().stored.clone();
        self.core
            .commit_agent(saved, state, changes, None, json!({}))
            .unwrap();
    }

    fn cursor(&mut self) -> u64 {
        self.core.store.replay(0).unwrap().0
    }

    /// The `pane.update` events after `cursor`, and every method of the commit.
    fn after(&mut self, cursor: u64) -> (Vec<Value>, Vec<String>) {
        let events = self.core.store.replay(cursor).unwrap().1;
        let methods = events.iter().map(|event| event.method.clone()).collect();
        let updates = events
            .into_iter()
            .filter(|event| event.method == "pane.update")
            .map(|event| event.params)
            .collect();
        (updates, methods)
    }

    fn call(&mut self, method: &str, extra: Value) -> crate::errors::Result<Value> {
        let mut params = json!({"paneId":PANE,"idempotencyKey":format!("key-{method}")});
        params.as_object_mut().unwrap().extend(
            extra
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        if method == "pane.close" {
            self.core.close_agent(&self.connection, &params)
        } else {
            self.core
                .agent_method(&self.connection, method, &params, Vec::new())
        }
    }

    fn finish(self) {
        crate::test_paths::remove_after(self.core, self.directory);
    }
}

fn stream_a_message(core: &Core, state: &mut State, changes: &mut Changes) {
    let chunk = TextChunk {
        text: "still streaming",
        message_id: None,
        subagent: None,
    };
    transcript::append_text(&core.store, PANE, state, 1, false, chunk, changes).unwrap();
}

fn start_a_subagent(core: &Core, state: &mut State, changes: &mut Changes) {
    let spawned = Spawned {
        session_id: "child".into(),
        name: "Helper".into(),
        task: "Look around".into(),
    };
    let context = Spawn {
        owner: Scope::Parent,
        hidden: false,
        timestamp: None,
    };
    subagents::spawned(&core.store, PANE, 1, state, spawned, context, changes).unwrap();
}

fn assert_each_names_the_person(updates: &[Value], kinds: &[&str]) {
    let found: Vec<_> = updates
        .iter()
        .map(|update| update["kind"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(found, kinds, "{updates:?}");
    for update in updates {
        assert_eq!(update["actor"]["role"], "human_local", "{update}");
    }
}

/// Stages `open` (what the pane had under way), closes the pane as the person, and returns the
/// `pane.update` events of the close, checked to name the closer and to come with `pane.closed`.
fn close_with(open: fn(&Core, &mut State, &mut Changes), kinds: &[&str]) -> Vec<Value> {
    let mut rig = Rig::new();
    rig.stage(open);
    let cursor = rig.cursor();

    rig.call("pane.close", json!({})).unwrap();

    let (updates, methods) = rig.after(cursor);
    assert_each_names_the_person(&updates, kinds);
    assert!(methods.contains(&"pane.closed".to_owned()), "{methods:?}");
    rig.finish();
    updates
}

#[test]
fn closing_a_pane_while_a_message_is_streaming_completes_it_under_the_closers_name() {
    let updates = close_with(stream_a_message, &["agent_message_chunk"]);
    assert_eq!(updates[0]["complete"], true);
}

#[test]
fn closing_a_pane_while_a_subagent_runs_disconnects_it_under_the_closers_name() {
    let updates = close_with(start_a_subagent, &["subagent"]);
    assert_eq!(updates[0]["subagent"]["status"], "disconnected");
}

#[test]
fn a_prompt_sent_while_the_pane_signs_in_is_noted_under_the_senders_name() {
    let mut rig = Rig::new();
    rig.stage(|_, state, _| {
        state.phase = Phase::NeedsLogin;
        state.login_pane_id = Some("login-pane".into());
    });
    let cursor = rig.cursor();

    rig.call("pane.prompt", json!({"text":"hello"})).unwrap();

    let (updates, _) = rig.after(cursor);
    assert_each_names_the_person(&updates, &["system_message"]);
    rig.finish();
}
