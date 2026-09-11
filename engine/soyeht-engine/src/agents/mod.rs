pub mod authentication;
pub mod catalog;
pub mod configuration;
pub mod delivery;
pub mod driver;
pub mod effects;
pub mod failures;
pub mod filesystem;
pub mod forms;
pub mod lifecycle;
pub mod methods;
pub mod pending;
pub mod questions;
pub mod recovery;
pub mod registry;
pub mod responses;
pub mod state;
pub mod terminals;
pub mod transcript;
pub mod updates;

use crate::{
    core::{Core, write_pane},
    errors::{self, Result},
    model::StoredPane,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::{Message, rpc::Decoder};
use soyeht_protocol::{ErrorKind, ProbeCapability, TranscriptMessage};
use soyeht_store::{
    agents::{self, NativeRequest, Outgoing},
    database::{Change, Mutation},
};
use state::State;
use uuid::Uuid;

pub struct Running {
    pub state: State,
    pub decoder: Decoder,
}
impl Running {
    pub fn new(state: State) -> Self {
        Self {
            decoder: Decoder::new(state.acknowledged_offset),
            state,
        }
    }
}

#[derive(Default)]
pub struct Changes {
    pub attention: Vec<soyeht_protocol::AttentionItem>,
    pub clear_attention: bool,
    pub remove_attention: Vec<String>,
    pub clear_failed_probes: bool,
    pub completed_user: Option<pending::Completion>,
    pub replay_messages: bool,
    pub clear_replay: bool,
    pub publish_replay: bool,
    pub retire_instance: Option<(String, String)>,
    pub messages: Vec<TranscriptMessage>,
    pub received: Vec<NativeRequest>,
    pub answers: Vec<(String, Value)>,
    pub outgoing: Vec<Message>,
    pub probes: Vec<ProbeCapability>,
    pub events: Vec<(String, Value)>,
}

pub fn native_error(error: soyeht_acp::Error) -> soyeht_protocol::RpcError {
    errors::error(ErrorKind::InvalidParams, error.0)
}

impl Core {
    pub fn agent_state(&self, id: &str) -> Result<State> {
        if self.panes.get(id).is_none_or(|p| p.stored.closed) {
            return Err(errors::error(ErrorKind::NotFound, "Pane not found"));
        }
        self.agents
            .get(id)
            .map(|agent| agent.state.clone())
            .ok_or_else(|| errors::error(ErrorKind::InvalidParams, "Agent pane required"))
    }

    /// No in-memory state or native bytes change until the whole transaction
    /// commits. `user` retains the existing client idempotency and actor scope.
    pub fn commit_agent(
        &mut self,
        mut saved: StoredPane,
        state: State,
        changes: Changes,
        user: Option<(&str, &str, &Value)>,
        result: Value,
    ) -> Result<Mutation> {
        let id = saved.pane.pane_id.clone();
        saved.pane.agent = Some(state.public(self.now_ms()));
        let old_queue = self
            .agents
            .get(&id)
            .map(|a| a.state.queue.clone())
            .unwrap_or_default();
        let mut outgoing = Vec::new();
        for message in changes.outgoing {
            outgoing.push(Outgoing {
                operation_key: Uuid::new_v4().to_string(),
                pane_id: id.clone(),
                generation: saved.pane.generation,
                instance_id: saved.instance_id.clone().ok_or_else(|| {
                    errors::error(ErrorKind::PtydUnavailable, "Agent process is not adopted")
                })?,
                supervisor_id: saved.supervisor_id.clone().ok_or_else(|| {
                    errors::error(
                        ErrorKind::PtydUnavailable,
                        "Agent supervisor is not adopted",
                    )
                })?,
                bytes: message.encode().map_err(native_error)?,
                accepted_bytes: 0,
                attempt: 0,
            });
        }
        let mut events = changes.events;
        if self.panes.get(&id).is_none_or(|running| {
            running.stored.pane.state != saved.pane.state
                || running.stored.pane.source != saved.pane.source
                || running.stored.pane.confidence != saved.pane.confidence
                || running.stored.pane.phrase != saved.pane.phrase
        }) {
            events.push((
                "pane.state_changed".into(),
                crate::core::state_event(&saved),
            ));
        }
        if self
            .panes
            .get(&id)
            .is_none_or(|running| running.stored.pane != saved.pane)
        {
            events.push(("pane.updated".into(), json!({"pane":saved.pane})));
        }
        let change = |tx: &Change<'_>| {
            if let Some(completion) = &changes.completed_user {
                let changed = tx.update_idempotency_result(
                    &completion.user.scope,
                    &completion.user.key,
                    &completion.stored(),
                )?;
                if !changed {
                    return Err(soyeht_store::database::Error::Invalid(
                        "Native operation has no durable caller",
                    ));
                }
            }
            if let Some((instance, supervisor)) = changes.retire_instance {
                tx.retire_agent_outbox_process(&id, &instance, &supervisor)?;
            }
            write_pane(tx, &saved)?;
            agents::save_runtime(tx, &id, saved.pane.generation, &state)?;
            let attention_changed = changes.clear_attention
                || !changes.attention.is_empty()
                || !changes.remove_attention.is_empty();
            if changes.clear_attention {
                tx.clear_attention(&id)?;
            }
            for attention in changes.remove_attention {
                tx.remove_attention(&id, &attention)?;
            }
            for attention in changes.attention {
                agents::save_attention(tx, &saved.pane.workspace_id, &attention)?;
            }
            if attention_changed {
                let items = agents::attention_in_change(tx, &saved.pane.workspace_id)?;
                events.push(("attention.changed".into(),json!({"workspaceId":saved.pane.workspace_id,"count":items.len(),"items":items})));
            }
            if changes.clear_failed_probes {
                tx.clear_failed_probes(&state.fingerprint)?;
            }
            if changes.clear_replay {
                agents::discard_replay(tx, &id)?;
            }
            for prompt in &old_queue {
                if !state.queue.iter().any(|p| p.prompt_id == prompt.prompt_id) {
                    agents::remove_prompt(tx, &id, &prompt.prompt_id)?;
                }
            }
            for prompt in &state.queue {
                if !old_queue.iter().any(|p| p.prompt_id == prompt.prompt_id) {
                    agents::enqueue_prompt(tx, &id, prompt)?;
                }
            }
            for request in changes.received {
                agents::receive(tx, &request)?;
            }
            for (request, result) in changes.answers {
                agents::answer(tx, &request, &result)?;
            }
            for message in changes.messages {
                if changes.replay_messages {
                    agents::stage_message(tx, &id, &message)?;
                } else {
                    agents::save_message(tx, &id, &message)?;
                }
            }
            if changes.publish_replay {
                agents::publish_replay(tx, &id, saved.pane.generation)?;
            }
            for output in outgoing {
                agents::enqueue(tx, &output)?;
            }
            for probe in changes.probes {
                agents::save_probe(tx, &state.fingerprint, &probe)?;
            }
            Ok((result, events))
        };
        let mutation = if let Some((connection, method, params)) = user {
            self.mutate(connection, method, params, change)?
        } else {
            let mutation = StorePort::mutate(
                &mut self.store,
                "engine-agent",
                &Uuid::new_v4().to_string(),
                "transition",
                change,
            )
            .map_err(errors::storage)?;
            self.publish(&mutation.events);
            mutation
        };
        if !mutation.repeated {
            if let Some(completion) = &changes.completed_user {
                self.publish_native_completion(completion);
            }
            self.panes.get_mut(&id).expect("known pane").stored = saved;
            if let Some(running) = self.agents.get_mut(&id) {
                running.state = state;
            } else {
                self.agents.insert(id, Running::new(state));
            }
        }
        Ok(mutation)
    }
}
