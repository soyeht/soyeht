//! The ceiling an agent that dispatches work puts on the permission mode of that work.
//!
//! A chief writes tasks and starts the executors that run them. Like a helper started by an
//! agent, an executor gets the permissions of the agent that started it, never more: the mode
//! of the executor ranks by the `kind` its own harness announces, and it is either below the
//! rank of the chief's mode or the very same mode on the very same harness (a rank groups modes
//! that differ). A person is not capped. Full access stays a person's decision
//! (`Core::require_person_for_full_access`).
//!
//! The request is checked here, before anything is reserved. The pane then keeps the ceiling
//! (`agents::mode_ceiling`) and checks the mode its harness actually runs each time a session
//! opens, since a harness drops a mode it does not offer and starts in its own.

use super::rules::requested_executor;
use crate::{
    agents::mode_ceiling::ModeCeiling,
    core::Core,
    errors::{self, Result},
};
use soyeht_protocol::{Actor, TaskCreateParams, TaskExecutor};

fn refused() -> errors::Error {
    errors::forbidden_role("A task cannot start in a mode above its dispatcher's own")
}

impl Core {
    /// The ceiling the agent bound to `actor` puts on what it starts: the lowest, for an agent
    /// the engine does not know.
    fn dispatcher_ceiling(&self, actor: &Actor) -> ModeCeiling {
        actor
            .pane_id
            .as_deref()
            .and_then(|pane| self.agents.running.get(pane))
            .map_or_else(ModeCeiling::unknown, |agent| {
                ModeCeiling::of(&agent.state, false)
            })
    }

    /// Refuses `mode` on `harness` unless it stays within `ceiling`. The mode is ranked by what
    /// a running agent of that harness announces for it (the most permissive when several do);
    /// a mode that harness has not announced cannot be shown to stay within the ceiling.
    fn require_mode_within(&self, ceiling: &ModeCeiling, harness: &str, mode: &str) -> Result<()> {
        let kinds: Vec<_> = self
            .agents
            .running
            .values()
            .filter(|agent| agent.state.launch.id == harness)
            .filter_map(|agent| agent.state.options.mode())
            .flat_map(|option| &option.options)
            .filter(|choice| choice.value == mode)
            .map(|choice| choice.kind.as_deref())
            .collect();
        if !kinds.is_empty()
            && kinds
                .iter()
                .all(|kind| ceiling.allows(harness, mode, *kind))
        {
            return Ok(());
        }
        Err(refused())
    }

    /// Refuses a task creation by an agent whose executor asks for a mode above the agent's.
    /// The executor of a task that is not dispatched yet may leave its harness to the claimer,
    /// whose harness announces the modes; such a task cannot name a mode.
    pub(crate) fn require_created_task_within_dispatcher(
        &self,
        actor: &Actor,
        params: &TaskCreateParams,
    ) -> Result<()> {
        if actor.role.is_person() {
            return Ok(());
        }
        let Some(executor) = requested_executor(params) else {
            return Ok(());
        };
        if params.dispatch == Some(true) {
            let mut preset = self.resolve_new_preset(&executor)?;
            return self.cap_preset_mode(actor, &mut preset).map(drop);
        }
        match (&executor.harness, &executor.permission_mode) {
            (_, None) => Ok(()),
            (Some(harness), Some(mode)) => {
                self.require_mode_within(&self.dispatcher_ceiling(actor), harness, mode)
            }
            (None, Some(_)) => Err(errors::forbidden_role(
                "Name the harness of the executor to give a task a permission mode",
            )),
        }
    }

    /// Fixes the mode of an executor an agent is about to start, then checks it against the
    /// agent's own, and answers the ceiling the executor's pane keeps. A preset that names no
    /// mode, with no harness default either, would start in a mode that cannot be ranked before
    /// the session announces its modes: it takes the agent's own current mode when the executor
    /// runs on the agent's harness, and is refused otherwise. A person's preset is left as it
    /// is, with no ceiling.
    pub(crate) fn cap_preset_mode(
        &self,
        actor: &Actor,
        preset: &mut TaskExecutor,
    ) -> Result<Option<ModeCeiling>> {
        if actor.role.is_person() {
            return Ok(None);
        }
        let ceiling = self.dispatcher_ceiling(actor);
        if self.new_pane_mode(preset)?.is_none() {
            let agent = actor
                .pane_id
                .as_deref()
                .and_then(|pane| self.agents.running.get(pane))
                .map(|agent| &agent.state)
                .filter(|state| preset.harness.as_deref() == Some(state.launch.id.as_str()));
            let Some(mode) = agent.and_then(|state| state.options.current_mode()) else {
                return Err(errors::forbidden_role(
                    "Name a permission mode for the executor; its harness default cannot be checked before it starts",
                ));
            };
            preset.permission_mode = Some(mode.to_owned());
        }
        let (Some(harness), Some(mode)) = (preset.harness.as_deref(), self.new_pane_mode(preset)?)
        else {
            return Err(refused());
        };
        self.require_mode_within(&ceiling, harness, &mode)?;
        Ok(Some(ceiling))
    }
}

#[cfg(test)]
mod tests;
