use super::{Changes, native_error, state::Phase};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::json;
use soyeht_protocol::{PaneState, StateSource, ndjson::Decoder};
use soyeht_ptyd::wire::{Instance, ProcessKind, Spawn};

impl Core {
    pub(crate) fn recover_missing_agent(&mut self, id: &str) -> Result<()> {
        self.recover_missing_agent_after(id, None)
    }

    /// [`Core::recover_missing_agent`] for an agent whose process is being closed and replaced:
    /// the new process starts once the close of `after` has been sent.
    pub(crate) fn recover_missing_agent_after(
        &mut self,
        id: &str,
        after: Option<&str>,
    ) -> Result<()> {
        let mut saved = self.panes[id].stored.clone();
        let mut state = self.agent_state(id)?;
        if state.phase == Phase::RestartAfterProbe {
            return self.restart_after_probe(id);
        }
        if saved.instance_id.is_none() {
            return self.spawn_after(id, after);
        }
        let mut changes = Changes {
            retire_instance: saved.instance_id.clone().zip(saved.supervisor_id.clone()),
            clear_replay: true,
            ..Changes::default()
        };
        super::questions::decide_open_as_system(
            &self.store,
            id,
            &mut state,
            &mut changes,
            &self.timestamp(),
        )?;
        // The old process is absent from the reconciled supervisor registry.
        // Close its decisions locally, without delivering them to a new process.
        changes.outgoing.clear();
        super::transcript::close(&self.store, id, &mut state, &mut changes)?;
        super::failures::fail_work_in_flight(
            &mut state,
            &mut changes,
            id,
            "Agent process was lost before confirmation",
        )?;
        saved.pane.state = PaneState::Recovering;
        saved.pane.phrase = "Recovering native agent session".into();
        state.phase = Phase::Recovering;
        // Finish records under their original generation before incrementing it.
        // A crash between these transactions repeats no completed decision.
        self.commit_agent(saved.clone(), state.clone(), changes, None, json!({}))?;
        saved.pane.generation = saved
            .pane
            .generation
            .checked_add(1)
            .ok_or_else(|| errors::internal("Pane generation exhausted"))?;
        saved.pane.stream_id = None;
        saved.pane.pid = None;
        saved.instance_id = None;
        saved.supervisor_id = None;
        saved.instance_key = uuid::Uuid::new_v4().to_string();
        saved.sequence_base = self.panes[id].screen.next_offset();
        state.client = Default::default();
        // The children of the lost agent are closed from the ledger, which outlives a lost link;
        // the ones of another supervisor are gone with it.
        for terminal in state.terminals.values() {
            state.retired_processes.extend(terminal.retirement());
        }
        state.terminals.clear();
        state.initialized = None;
        state.phase = Phase::Initializing;
        state.cancel_at_ms = None;
        state.initial_option = None;
        state.initial_config_failed = false;
        state.acknowledged_offset = 0;
        state.discard_until_newline = false;
        state.probe_started_ms = None;
        state.probe_session_id = None;
        state.replay_role = None;
        self.commit_agent(saved, state, Changes::default(), None, json!({}))?;
        errors::invariant(
            self.agents.get_mut(id),
            "Agent runtime disappeared during recovery",
        )?
        .decoder = Decoder::new(0);
        self.spawn_after(id, after)
    }

    /// The process the agent of pane `id` runs under the identity the pane has saved.
    pub(crate) fn agent_spawn_config(&self, id: &str) -> Spawn {
        let saved = &self.panes[id].stored;
        let state = &self.agents[id].state;
        let mut argv = vec![state.launch.command.clone()];
        argv.extend(state.launch.args.clone());
        Spawn {
            instance_key: saved.instance_key.clone(),
            generation: saved.pane.generation,
            kind: ProcessKind::PipesAcp,
            cwd: saved.pane.cwd.clone(),
            argv,
            env: super::environment::launch_environment_with(state.launch.environment.as_deref()),
            cols: saved.pane.cols as u16,
            rows: saved.pane.rows as u16,
        }
    }

    pub fn adopt_agent(&mut self, id: &str, instance: &Instance) -> Result<()> {
        if instance.spawn.kind != ProcessKind::PipesAcp {
            return Err(errors::ptyd_unavailable(
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
