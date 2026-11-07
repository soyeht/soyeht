//! The ceiling an agent that dispatches work puts on the permission mode of that work.
//!
//! A chief writes tasks and starts the executors that run them. It may not start work with more
//! freedom than it has: its own permission mode, ranked by the mode `kind`, caps the mode of
//! every executor it starts. A person is not capped. Full access stays a person's decision
//! (`Core::require_person_for_full_access`).

use crate::{
    core::Core,
    errors::{self, Result},
};
use soyeht_acp::config::{FULL_ACCESS_KIND, mode_kind_rank};
use soyeht_protocol::{Actor, Role};

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
}

#[cfg(test)]
mod tests;
