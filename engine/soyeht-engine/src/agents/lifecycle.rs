use super::{Changes, native_error, state::Phase};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::json;
use soyeht_protocol::{ErrorKind, PaneState, StateSource};
use soyeht_ptyd::wire::{Instance, ProcessKind, Request, Response, Spawn};
use std::collections::BTreeMap;

impl Core {
    pub(crate) fn recover_missing_agent(&mut self, id: &str) -> Result<()> {
        let mut saved = self.panes[id].stored.clone();
        let mut state = self.agent_state(id)?;
        if state.phase == Phase::RestartAfterProbe {
            return self.restart_after_probe(id);
        }
        if saved.instance_id.is_none() {
            return self.spawn_agent(id);
        }
        let mut changes = Changes {
            retire_instance: saved.instance_id.clone().zip(saved.supervisor_id.clone()),
            clear_replay: true,
            ..Changes::default()
        };
        for request in self
            .store
            .agent_open_requests(id)
            .map_err(errors::storage)?
        {
            super::questions::decision(
                &mut state,
                &request,
                super::questions::Decision {
                    option: None,
                    fields: None,
                    by: "system",
                    expired: false,
                    timestamp: &self.timestamp(),
                },
                &mut changes,
            )?;
        }
        // The old process is absent from the reconciled supervisor registry.
        // Close its decisions locally, without delivering them to a new process.
        changes.outgoing.clear();
        if state.phase == Phase::Loading {
            state.open_messages.clear();
        } else {
            super::transcript::complete(&self.store, id, &mut state, &mut changes)?;
        }
        if let Some(prompt) = state.active.take() {
            changes.events.push((
                "pane.turn_ended".into(),
                json!({"paneId":id,"promptId":prompt.prompt_id,"stopReason":"error"}),
            ));
        }
        if let Some(operation) = state.operation.take() {
            changes.events.push(("pane.config_failed".into(),json!({"paneId":id,"configId":operation.config_id,"value":operation.value,"reason":"Agent process was lost before confirmation"})));
        }
        if let Some(user) = state.operation_user.take() {
            changes.completed_user = Some(super::pending::Completion {
                user,
                result: Err(errors::error(
                    ErrorKind::HarnessUnavailable,
                    "Agent process was lost before confirmation",
                )),
            });
        }
        saved.pane.state = PaneState::Recovering;
        saved.pane.phrase = "Recovering native agent session".into();
        state.phase = Phase::Recovering;
        // Finish records under their original generation before incrementing it.
        // A crash between these transactions repeats no completed decision.
        self.commit_agent(saved.clone(), state.clone(), changes, None, json!({}))?;
        saved.pane.generation =
            saved.pane.generation.checked_add(1).ok_or_else(|| {
                errors::error(ErrorKind::InternalError, "Pane generation exhausted")
            })?;
        saved.pane.stream_id = None;
        saved.pane.pid = None;
        saved.instance_id = None;
        saved.supervisor_id = None;
        saved.instance_key = uuid::Uuid::new_v4().to_string();
        saved.sequence_base = self.panes[id].screen.next_offset();
        state.client = Default::default();
        for terminal in state.terminals.values() {
            if terminal.supervisor_id.as_deref() == self.ptyd.as_ref().map(|p| p.supervisor_id())
                && let Some(instance_id) = &terminal.instance_id
            {
                self.pty_request(Request::Close {
                    instance_id: instance_id.clone(),
                })?;
            }
        }
        state.terminals.clear();
        state.initialized = None;
        state.phase = Phase::Initializing;
        state.cancel_at_ms = None;
        state.pending_policy = None;
        state.initial_option = None;
        state.initial_config_failed = false;
        state.acknowledged_offset = 0;
        state.discard_until_newline = false;
        state.probe_started_ms = None;
        state.probe_session_id = None;
        state.replay_role = None;
        self.commit_agent(saved, state, Changes::default(), None, json!({}))?;
        self.agents.get_mut(id).expect("agent").decoder = soyeht_acp::rpc::Decoder::new(0);
        self.spawn_agent(id)
    }

    pub fn spawn_agent(&mut self, id: &str) -> Result<()> {
        let saved = &self.panes[id].stored;
        let state = &self.agents[id].state;
        let mut argv = vec![state.launch.command.clone()];
        argv.extend(state.launch.args.clone());
        let config = Spawn {
            instance_key: saved.instance_key.clone(),
            generation: saved.pane.generation,
            kind: ProcessKind::PipesAcp,
            cwd: saved.pane.cwd.clone(),
            argv,
            env: BTreeMap::new(),
            cols: saved.pane.cols as u16,
            rows: saved.pane.rows as u16,
        };
        let Response::Instance(instance) = self.pty_request(Request::Spawn(config))? else {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Invalid agent spawn response",
            ));
        };
        self.adopt_agent(id, &instance)
    }

    pub fn adopt_agent(&mut self, id: &str, instance: &Instance) -> Result<()> {
        if instance.spawn.kind != ProcessKind::PipesAcp {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Agent adoption requires ACP pipes",
            ));
        }
        let mut saved = self.panes[id].stored.clone();
        let mut state = self.agent_state(id)?;
        saved.instance_id = Some(instance.instance_id.clone());
        saved.supervisor_id = Some(instance.supervisor_id.clone());
        saved.pane.pid = Some(instance.pid);
        saved.pane.source = StateSource::Protocol;
        saved.pane.confidence = 1.0;
        let mut changes = Changes::default();
        if instance.exit_code.is_some() {
            saved.pane.state = PaneState::Dead;
            saved.pane.phrase = "Agent process exited".into();
        } else if state.initialized.is_none() && state.client.pending.is_empty() {
            changes.outgoing.push(
                state
                    .client
                    .initialize(self.now_ms() as u64)
                    .map_err(native_error)?,
            );
            state.phase = Phase::Initializing;
        }
        // A pending initialize or prompt belongs to the live process. Adoption
        // restores its correlation; it never opens a replacement session.
        self.commit_agent(saved, state, changes, None, json!({}))?;
        // An exited process can still own buffered replies. The poller drains
        // them before it closes the process and settles its remaining requests.
        Ok(())
    }
}
