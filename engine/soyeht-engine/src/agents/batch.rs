//! The records one poll decoded for a pane, applied to its state in memory and committed once
//! (ADR 0020). A streamed reply is one record per chunk; a commit per record was an `fsync`, a
//! serialization of the whole agent state and an event per chunk, under the Core lock.
//!
//! Only `session/update` records that change nothing but the transcript are collected. Anything
//! else (a response, a request, a change that needs a commit of its own) ends the batch first, so
//! it is committed exactly as before, on top of the state the batch left.

use super::{Changes, state::State};
use crate::model::StoredPane;
use serde_json::Value;
use soyeht_protocol::{TranscriptMessage, event};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The `pane.update` kinds whose event carries the whole message so far, so a later event of the
/// same message replaces an earlier one. `terminal_output` is left out: it carries a piece.
const WHOLE_STATE: [&str; 8] = [
    "agent_message_chunk",
    "agent_thought_chunk",
    "system_message",
    "tool_call",
    "tool_call_update",
    "plan",
    "subagent",
    "usage",
];

/// The messages of the records applied but not committed yet, by sequence, that the next record
/// reads before the store.
#[derive(Clone, Default)]
pub struct Earlier(Arc<BTreeMap<u64, TranscriptMessage>>);

impl Earlier {
    pub fn get(&self, sequence: u64) -> Option<TranscriptMessage> {
        self.0.get(&sequence).cloned()
    }
}

/// Whether `changes` holds only what a batch can carry: messages (with a sequence) and events.
pub fn transcript_only(changes: &Changes) -> bool {
    changes
        .messages
        .iter()
        .all(|message| message.seq.parse::<u64>().is_ok())
        && carries_nothing_else(changes)
}

fn carries_nothing_else(changes: &Changes) -> bool {
    let Changes {
        attention,
        clear_attention,
        remove_attention,
        clear_failed_probes,
        completed_user,
        replay_messages: _,
        clear_replay,
        publish_replay,
        retire_instance,
        messages: _,
        received,
        answers,
        outgoing,
        probes,
        events: _,
        model_refusal,
        sign_in,
        acknowledged_package,
        workspace_layout,
        closed_login_pane,
        message_rate,
        task_owner_loss,
        task_human_move,
        task_blocking,
        task_dispatch,
        task_feedback,
        task_editor_feedback,
        task_loop,
        editor_agent_write,
        earlier: _,
    } = changes;
    attention.is_empty()
        && !clear_attention
        && remove_attention.is_empty()
        && !clear_failed_probes
        && completed_user.is_none()
        && !clear_replay
        && !publish_replay
        && retire_instance.is_none()
        && received.is_empty()
        && answers.is_empty()
        && outgoing.is_empty()
        && probes.is_empty()
        && model_refusal.is_none()
        && sign_in.is_none()
        && acknowledged_package.is_none()
        && workspace_layout.is_none()
        && closed_login_pane.is_none()
        && message_rate.is_none()
        && task_owner_loss.is_none()
        && task_human_move.is_none()
        && task_blocking.is_none()
        && task_dispatch.is_none()
        && task_feedback.is_none()
        && task_editor_feedback.is_none()
        && task_loop.is_none()
        && editor_agent_write.is_none()
}

/// The records of one poll of a pane, applied in memory.
pub struct Batch {
    pub saved: StoredPane,
    pub state: State,
    earlier: Earlier,
    /// Whether the messages are staged for a native replay instead of saved.
    replay: bool,
    events: Vec<Option<(String, Value)>>,
    /// Where the last event of a whole-state message is in `events`, by kind and local ID.
    last_event: HashMap<(String, String), usize>,
}

impl Batch {
    /// A batch of the first record `takes` accepted, with the state and pane it left.
    pub fn start(saved: StoredPane, state: State, changes: Changes) -> Self {
        let mut batch = Self {
            saved,
            state,
            earlier: Earlier::default(),
            replay: changes.replay_messages,
            events: Vec::new(),
            last_event: HashMap::new(),
        };
        batch.merge(changes);
        batch
    }

    pub fn earlier(&self) -> Earlier {
        self.earlier.clone()
    }

    /// Whether `changes` can join the batch: only a transcript change, of the same kind of
    /// message, and a message already in the batch is only replaced by a later state of itself
    /// (same identity, not completed yet). Anything else is committed on its own, where the
    /// store checks it against what it holds.
    pub fn takes(&self, changes: &Changes) -> bool {
        carries_nothing_else(changes)
            && (self.earlier.0.is_empty() || changes.replay_messages == self.replay)
            && changes.messages.iter().all(|next| {
                let Ok(sequence) = next.seq.parse() else {
                    return false;
                };
                self.earlier.0.get(&sequence).is_none_or(|prior| {
                    !prior.complete
                        && prior.local_id == next.local_id
                        && prior.generation == next.generation
                        && prior.kind == next.kind
                        && prior.role == next.role
                        && prior.prompt_id == next.prompt_id
                })
            })
    }

    /// Adds a record `takes` accepted, with the state and pane it left.
    pub fn absorb(&mut self, changes: Changes, saved: StoredPane, state: State) {
        if self.earlier.0.is_empty() {
            self.replay = changes.replay_messages;
        }
        self.merge(changes);
        self.saved = saved;
        self.state = state;
    }

    fn merge(&mut self, mut changes: Changes) {
        // The record's own handle on the batch's messages must be gone before they change.
        changes.earlier = Earlier::default();
        let messages = Arc::make_mut(&mut self.earlier.0);
        for message in changes.messages {
            if let Ok(sequence) = message.seq.parse() {
                messages.insert(sequence, message);
            }
        }
        for (name, params) in changes.events {
            let key = (name == event::PANE_UPDATE)
                .then(|| {
                    let kind = params["kind"].as_str()?;
                    let local = params["localId"].as_str()?;
                    WHOLE_STATE
                        .contains(&kind)
                        .then(|| (kind.to_owned(), local.to_owned()))
                })
                .flatten();
            if let Some(key) = &key
                && let Some(at) = self.last_event.insert(key.clone(), self.events.len())
            {
                self.events[at] = None;
            }
            self.events.push(Some((name, params)));
        }
    }

    /// What to commit: the state and pane the last record left, with the messages and events of
    /// all of them, in order.
    pub fn finish(self) -> (StoredPane, State, Changes) {
        let messages = Arc::try_unwrap(self.earlier.0)
            .unwrap_or_else(|shared| (*shared).clone())
            .into_values()
            .collect();
        let changes = Changes {
            replay_messages: self.replay,
            messages,
            events: self.events.into_iter().flatten().collect(),
            ..Changes::default()
        };
        (self.saved, self.state, changes)
    }
}

#[cfg(test)]
mod tests;
