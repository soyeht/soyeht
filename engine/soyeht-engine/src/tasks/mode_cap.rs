//! The ceiling an agent that dispatches work puts on the permission mode of that work.
//!
//! A chief writes tasks and starts the executors that run them. It may not start work with more
//! freedom than it has: its own permission mode, ranked by the mode `kind`, caps the mode of
//! every executor it starts. A person is not capped. Full access stays a person's decision
//! (`Core::require_person_for_full_access`).

use crate::{
    agents::configuration::{ACCEPT_EDITS, ASK},
    core::Core,
    errors::{self, Result},
};
use soyeht_acp::config::{FULL_ACCESS_KIND, mode_kind_rank};
use soyeht_protocol::{Actor, Role, TaskExecutor};

impl Core {
    /// The rank of the mode the agent bound to `actor` is in; a mode that cannot be ranked, or
    /// an agent that announces none, counts as the lowest.
    fn dispatcher_mode_rank(&self, actor: &Actor) -> usize {
        let Some(state) = actor
            .pane_id
            .as_deref()
            .and_then(|pane| self.agents.get(pane))
            .map(|agent| &agent.state)
        else {
            return 0;
        };
        let current = state.options.current_mode();
        state
            .options
            .mode()
            .and_then(|option| {
                option
                    .options
                    .iter()
                    .find(|choice| Some(choice.value.as_str()) == current)
            })
            .and_then(|choice| mode_kind_rank(choice.kind.as_deref()))
            .unwrap_or(0)
    }

    /// The rank of `mode` as a running agent announces it (the most permissive when several
    /// announce it), else full access for a mode known to approve everything. `None` when no
    /// agent in the engine has announced the mode.
    fn requested_mode_rank(&self, mode: &str) -> Option<usize> {
        self.agents
            .values()
            .filter_map(|agent| agent.state.options.mode())
            .flat_map(|option| &option.options)
            .filter(|choice| choice.value == mode)
            .filter_map(|choice| mode_kind_rank(choice.kind.as_deref()))
            .max()
            .or_else(|| {
                self.is_full_access_mode(mode)
                    .then(|| mode_kind_rank(Some(FULL_ACCESS_KIND)))
                    .flatten()
            })
            // The engine's own modes are standard whether or not a pane that uses them is running.
            .or_else(|| {
                [ASK, ACCEPT_EDITS]
                    .contains(&mode)
                    .then(|| mode_kind_rank(Some("standard")))
                    .flatten()
            })
    }

    /// Whether `mode` is positively known to approve less than everything. A mode that no
    /// running agent announced, and that the engine does not know, is not.
    pub(crate) fn mode_is_known_below_full_access(&self, mode: &str) -> bool {
        let full = mode_kind_rank(Some(FULL_ACCESS_KIND)).unwrap_or(usize::MAX);
        self.requested_mode_rank(mode)
            .is_some_and(|rank| rank < full)
    }

    /// Whether the pane's agent is in a mode that approves everything now: the mode it runs
    /// in, or the one it was asked to start in while its modes are not announced yet.
    pub(crate) fn agent_in_full_access(&self, state: &crate::agents::state::State) -> bool {
        [state.options.current_mode(), state.mode.as_deref()]
            .into_iter()
            .flatten()
            .any(|mode| self.is_full_access_mode(mode))
    }

    /// Refuses work an agent starts in a mode above its own. A person's request is not capped.
    /// A requested mode no agent has announced cannot be shown to be within the cap, so it is
    /// refused too, unless the dispatcher is in the most permissive rank already.
    pub(crate) fn require_modes_within_dispatcher<'a>(
        &self,
        actor: &Actor,
        modes: impl IntoIterator<Item = Option<&'a str>>,
    ) -> Result<()> {
        if matches!(actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Ok(());
        }
        let cap = self.dispatcher_mode_rank(actor);
        let highest = mode_kind_rank(Some(FULL_ACCESS_KIND)).unwrap_or(usize::MAX);
        for mode in modes.into_iter().flatten() {
            let within = match self.requested_mode_rank(mode) {
                Some(rank) => rank <= cap,
                None => cap >= highest,
            };
            if !within {
                return Err(errors::forbidden_role(
                    "A task cannot start in a mode above its dispatcher's own",
                ));
            }
        }
        Ok(())
    }

    /// Fixes the mode of an executor an agent is about to start, then checks it against the
    /// agent's own. A preset that names no mode, with no harness default either, would start in
    /// a mode that cannot be ranked before the session announces its modes: it takes the
    /// agent's own current mode when the executor runs on the agent's harness, and is refused
    /// otherwise. A person's preset is left as it is.
    pub(crate) fn cap_preset_mode(&self, actor: &Actor, preset: &mut TaskExecutor) -> Result<()> {
        if matches!(actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Ok(());
        }
        if self.new_pane_mode(preset)?.is_none() {
            let agent = actor
                .pane_id
                .as_deref()
                .and_then(|pane| self.agents.get(pane))
                .map(|agent| &agent.state)
                .filter(|state| preset.harness.as_deref() == Some(state.launch.id.as_str()));
            let Some(mode) = agent.and_then(|state| state.options.current_mode()) else {
                return Err(errors::forbidden_role(
                    "Name a permission mode for the executor; its harness default cannot be checked before it starts",
                ));
            };
            preset.permission_mode = Some(mode.to_owned());
        }
        self.require_modes_within_dispatcher(actor, [self.new_pane_mode(preset)?.as_deref()])
    }
}

#[cfg(test)]
mod tests;
