pub mod attachments;
pub mod authentication;
pub mod catalog;
pub mod commands;
pub mod configuration;
pub mod continuation;
pub mod custom;
pub mod delivery;
pub mod driver;
pub mod effects;
pub mod environment;
pub mod failures;
pub mod filesystem;
pub mod forms;
pub mod hooks;
pub mod idle;
pub mod lifecycle;
pub mod mcp;
pub mod methods;
pub mod naming;
pub mod opening;
pub mod orphans;
pub mod peer;
pub mod pending;
pub mod questions;
pub mod recovery;
pub mod registry;
pub mod responses;
pub mod state;
pub mod subagents;
pub mod switching;
pub mod terminals;
pub mod transcript;
pub mod updates;

use crate::{
    core::{Core, persist_pane},
    errors::{self, Result},
    model::StoredPane,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_acp::Message;
use soyeht_protocol::{
    HarnessSetupReason, HarnessSetupState, ProbeCapability, PromptItem, TranscriptMessage,
    Workspace, ndjson::Decoder,
};
use soyeht_store::{
    agents::{self, AgentModelRefusal, NativeRequest, Outgoing},
    database::{Change, Mutation},
};
use state::State;
use std::collections::VecDeque;
use uuid::Uuid;

pub struct Running {
    pub state: State,
    pub decoder: Decoder,
    pub(crate) idle: idle::Idle,
}
impl Running {
    pub fn new(state: State) -> Self {
        Self {
            decoder: Decoder::new(state.acknowledged_offset),
            state,
            idle: Default::default(),
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
    pub model_refusal: Option<AgentModelRefusal>,
    pub workspace_layout: Option<Workspace>,
    /// A sign-in terminal closed with this commit once the agent confirmed sign-in.
    pub closed_login_pane: Option<StoredPane>,
    pub(crate) message_rate: Option<(String, String, i64, i64)>,
    pub(crate) task_owner_loss: Option<crate::tasks::TaskOwnerLoss>,
    pub(crate) task_human_move: Option<crate::tasks::TaskHumanMove>,
    pub(crate) task_blocking: Option<crate::tasks::blocking::TaskBlockingChange>,
    pub(crate) task_dispatch: Option<crate::tasks::dispatching::TaskDispatchChange>,
    pub(crate) task_feedback: Option<crate::tasks::feedback::TaskFeedbackChange>,
    pub(crate) task_editor_feedback: Option<crate::tasks::feedback::TaskEditorFeedbackChange>,
    pub(crate) task_loop: Option<crate::tasks::looping::TaskLoopChange>,
    pub(crate) editor_agent_write: Option<crate::editor::agent_review::AgentWriteChange>,
}

pub fn native_error(error: soyeht_acp::Error) -> errors::Error {
    errors::invalid(error.to_string())
}

fn outgoing_messages(
    pane: &str,
    saved: &StoredPane,
    messages: &[Message],
) -> Result<Vec<Outgoing>> {
    if messages.is_empty() {
        return Ok(Vec::new());
    }
    let instance_id = saved
        .instance_id
        .as_ref()
        .ok_or_else(|| errors::ptyd_unavailable("Agent process is not adopted"))?;
    let supervisor_id = saved
        .supervisor_id
        .as_ref()
        .ok_or_else(|| errors::ptyd_unavailable("Agent supervisor is not adopted"))?;
    messages
        .iter()
        .map(|message| {
            Ok(Outgoing {
                operation_key: Uuid::new_v4().to_string(),
                pane_id: pane.into(),
                generation: saved.pane.generation,
                instance_id: instance_id.clone(),
                supervisor_id: supervisor_id.clone(),
                bytes: message.encode().map_err(native_error)?,
                accepted_bytes: 0,
                attempt: 0,
            })
        })
        .collect()
}

struct AgentPersistence<'a> {
    id: &'a str,
    saved: &'a StoredPane,
    state: &'a State,
    old_queue: &'a VecDeque<PromptItem>,
    changes: &'a Changes,
    outgoing: &'a [Outgoing],
}

fn persist_agent_metadata(
    tx: &Change<'_>,
    changes: &Changes,
) -> soyeht_store::database::Result<()> {
    if let Some(refusal) = &changes.model_refusal {
        agents::save_model_refusal(tx, refusal)?;
    }
    if let Some((sender, target, at_ms, since_ms)) = &changes.message_rate {
        agents::record_message(tx, sender, target, *at_ms, *since_ms)?;
    }
    Ok(())
}

fn persist_harness_readiness(
    tx: &Change<'_>,
    state: &State,
    changes: &Changes,
) -> soyeht_store::database::Result<()> {
    if let Some((status, reason)) = readiness_after_changes(state, changes) {
        agents::save_harness_readiness(tx, &state.launch.id, status, reason, state.last_update_ms)?;
    }
    Ok(())
}

fn readiness_after_changes(
    state: &State,
    changes: &Changes,
) -> Option<(HarnessSetupState, Option<HarnessSetupReason>)> {
    let has_event = |name: &str| changes.events.iter().any(|(event, _)| event == name);
    if state.phase == state::Phase::Faulted {
        Some((
            HarnessSetupState::CheckSetup,
            Some(if state.initialized.is_some() {
                HarnessSetupReason::SessionFailed
            } else {
                HarnessSetupReason::LaunchFailed
            }),
        ))
    } else if state.auth_required
        && (has_event("pane.session_started")
            || has_event("pane.needs_login")
            || has_event("pane.auth_finished"))
    {
        Some((
            HarnessSetupState::SignIn,
            Some(HarnessSetupReason::AuthenticationRequired),
        ))
    } else if has_event("pane.session_started")
        || has_event("pane.auth_finished")
        || has_event("pane.turn_ended")
    {
        Some((HarnessSetupState::Ready, None))
    } else {
        None
    }
}

fn persist_agent_checkpoint(
    tx: &Change<'_>,
    id: &str,
    generation: u32,
    state: &State,
    changes: &Changes,
) -> soyeht_store::database::Result<()> {
    agents::save_runtime(tx, id, generation, state)?;
    persist_harness_readiness(tx, state, changes)
}

fn persist_completion(
    tx: &Change<'_>,
    completion: Option<&pending::Completion>,
) -> soyeht_store::database::Result<()> {
    if let Some(completion) = completion {
        let changed = tx.update_idempotency_result(
            &completion.user.scope,
            &completion.user.key,
            &completion.stored(),
        )?;
        if !changed {
            return Err(soyeht_store::database::Error::NativeEffectNotFound);
        }
    }
    Ok(())
}

fn persist_workspace_layout(
    tx: &Change<'_>,
    workspace: Option<&Workspace>,
) -> soyeht_store::database::Result<()> {
    if let Some(workspace) = workspace {
        tx.update_workspace(workspace)?;
    }
    Ok(())
}

impl AgentPersistence<'_> {
    fn apply(
        self,
        tx: &Change<'_>,
        mut events: Vec<(String, Value)>,
        result: Value,
    ) -> soyeht_store::database::Result<(Value, Vec<(String, Value)>)> {
        persist_completion(tx, self.changes.completed_user.as_ref())?;
        if let Some((instance, supervisor)) = &self.changes.retire_instance {
            tx.retire_agent_outbox_process(self.id, instance, supervisor)?;
        }
        if let Some(loss) = &self.changes.task_owner_loss {
            loss.persist(tx)?;
            events.extend(loss.events());
        }
        if let Some(movement) = &self.changes.task_human_move {
            movement.persist(tx)?;
            events.extend(movement.events());
        }
        if let Some(blocking) = &self.changes.task_blocking {
            blocking.persist(tx)?;
        }
        if let Some(dispatch) = &self.changes.task_dispatch {
            dispatch.persist(tx)?;
        }
        if let Some(feedback) = &self.changes.task_feedback {
            feedback.persist(tx)?;
        }
        if let Some(feedback) = &self.changes.task_editor_feedback {
            feedback.persist(tx)?;
        }
        if let Some(task_loop) = &self.changes.task_loop {
            task_loop.persist(tx)?;
        }
        if let Some(write) = &self.changes.editor_agent_write {
            write.persist(tx)?;
        }
        persist_agent_metadata(tx, self.changes)?;
        persist_pane(tx, self.saved)?;
        if let Some(login) = &self.changes.closed_login_pane {
            persist_pane(tx, login)?;
        }
        persist_workspace_layout(tx, self.changes.workspace_layout.as_ref())?;
        persist_agent_checkpoint(
            tx,
            self.id,
            self.saved.pane.generation,
            self.state,
            self.changes,
        )?;
        let attention_changed = self.changes.clear_attention
            || !self.changes.attention.is_empty()
            || !self.changes.remove_attention.is_empty();
        if self.changes.clear_attention {
            tx.clear_attention(self.id)?;
        }
        for attention in &self.changes.remove_attention {
            tx.remove_attention(self.id, attention)?;
        }
        for attention in &self.changes.attention {
            agents::save_attention(tx, &self.saved.pane.workspace_id, attention)?;
        }
        if attention_changed {
            let items = agents::attention_in_change(tx, &self.saved.pane.workspace_id)?;
            events.push(("attention.changed".into(),json!({"workspaceId":self.saved.pane.workspace_id,"count":items.len(),"items":items})));
        }
        if self.changes.clear_failed_probes {
            tx.clear_failed_probes(&self.state.fingerprint)?;
        }
        if self.changes.clear_replay {
            agents::discard_replay(tx, self.id)?;
        }
        for prompt in self.old_queue {
            if !self
                .state
                .queue
                .iter()
                .any(|item| item.prompt_id == prompt.prompt_id)
            {
                agents::remove_prompt(tx, self.id, &prompt.prompt_id)?;
            }
        }
        for prompt in &self.state.queue {
            if !self
                .old_queue
                .iter()
                .any(|item| item.prompt_id == prompt.prompt_id)
            {
                agents::enqueue_prompt(tx, self.id, prompt)?;
            }
        }
        for request in &self.changes.received {
            agents::receive(tx, request)?;
        }
        for (request, answer) in &self.changes.answers {
            agents::answer(tx, request, answer)?;
        }
        for message in &self.changes.messages {
            if self.changes.replay_messages {
                agents::stage_message(tx, self.id, message)?;
            } else {
                agents::save_message(tx, self.id, message)?;
            }
        }
        if self.changes.publish_replay {
            agents::publish_replay(tx, self.id, self.saved.pane.generation)?;
        }
        for output in self.outgoing {
            agents::enqueue(tx, output)?;
        }
        for probe in &self.changes.probes {
            agents::save_probe(tx, &self.state.fingerprint, probe)?;
        }
        Ok((result, events))
    }
}

impl Core {
    pub fn agent_state(&self, id: &str) -> Result<State> {
        if self.panes.get(id).is_none_or(|p| p.stored.closed) {
            return Err(errors::not_found("Pane not found"));
        }
        self.agents
            .get(id)
            .map(|agent| agent.state.clone())
            .ok_or_else(|| errors::invalid("Agent pane required"))
    }

    pub(crate) fn replace_agent_state(&mut self, id: &str, state: State) -> Result<()> {
        let running = errors::invariant(self.agents.get_mut(id), "Agent runtime disappeared")?;
        running.state = state;
        running.idle.touch();
        Ok(())
    }

    fn install_committed_agent(&mut self, id: &str, saved: StoredPane, state: State) -> Result<()> {
        errors::invariant(
            self.panes.get_mut(id),
            "Agent pane disappeared before commit publication",
        )?
        .stored = saved;
        if let Some(running) = self.agents.get_mut(id) {
            running.state = state;
            running.idle.touch();
        } else {
            self.agents.insert(id.to_owned(), Running::new(state));
        }
        Ok(())
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
        let protocol_label_changed = self.panes.get(&id).is_none_or(|running| {
            running.stored.pane.state != saved.pane.state
                || running.stored.pane.source != saved.pane.source
                || running
                    .stored
                    .pane
                    .agent
                    .as_ref()
                    .zip(saved.pane.agent.as_ref())
                    .is_none_or(|(old, new)| {
                        old.harness != new.harness || old.version != new.version
                    })
        }) && saved.pane.source
            == soyeht_protocol::StateSource::Protocol;
        let old_queue = self
            .agents
            .get(&id)
            .map(|a| a.state.queue.clone())
            .unwrap_or_default();
        let outgoing = outgoing_messages(&id, &saved, &changes.outgoing)?;
        let mut events = changes.events.clone();
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
        let persistence = AgentPersistence {
            id: &id,
            saved: &saved,
            state: &state,
            old_queue: &old_queue,
            changes: &changes,
            outgoing: &outgoing,
        };
        let change = |tx: &Change<'_>| persistence.apply(tx, events, result);
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
            self.publish(&mutation.events)?;
            mutation
        };
        if !mutation.repeated {
            if let Some(completion) = &changes.completed_user {
                self.publish_native_completion(completion)?;
            }
            self.install_committed_agent(&id, saved, state)?;
            if let Some(login) = &changes.closed_login_pane {
                errors::invariant(
                    self.panes.get_mut(&login.pane.pane_id),
                    "Completed sign-in pane disappeared before commit publication",
                )?
                .stored = login.clone();
                self.finish_close(&login.pane.pane_id)?;
            }
            if protocol_label_changed && self.agent_captures.contains_key(&id) {
                self.record_agent_label(&id)?;
            }
            if let Some(write) = changes.editor_agent_write {
                self.install_agent_write(write);
            }
        }
        Ok(mutation)
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::{
        Changes, readiness_after_changes,
        state::{Phase, State},
    };
    use serde_json::json;
    use soyeht_protocol::{HarnessLaunch, HarnessSetupReason, HarnessSetupState};

    #[test]
    fn session_started_does_not_clear_a_prior_authentication_failure() {
        let mut state = State::new(
            HarnessLaunch {
                id: "fake".into(),
                name: "Fake".into(),
                command: "/fake".into(),
                args: vec![],
                version: "1".into(),
                environment: None,
            },
            "digest".into(),
            None,
        )
        .unwrap();
        state.phase = Phase::Ready;
        state.auth_required = true;
        let started = Changes {
            events: vec![("pane.session_started".into(), json!({}))],
            ..Changes::default()
        };
        assert_eq!(
            readiness_after_changes(&state, &started),
            Some((
                HarnessSetupState::SignIn,
                Some(HarnessSetupReason::AuthenticationRequired)
            ))
        );
        state.auth_required = false;
        let authenticated = Changes {
            events: vec![("pane.auth_finished".into(), json!({"ok":true}))],
            ..Changes::default()
        };
        assert_eq!(
            readiness_after_changes(&state, &authenticated),
            Some((HarnessSetupState::Ready, None))
        );
    }
}
