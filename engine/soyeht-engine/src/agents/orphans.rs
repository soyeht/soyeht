//! Agent processes that nothing in the engine owns. The supervisor keeps an
//! instance until it is closed, so one that the engine forgot (a pane row
//! removed, a retirement ledger lost with an old build) would live, with its
//! adapter tree, until the supervisor restarts. Reconciliation closes them.

use crate::{core::Core, errors::Result};
use soyeht_ptyd::wire::{Instance, ProcessKind, Request};
use std::collections::BTreeSet;

impl Core {
    /// Closes every open agent process in `instances` that no pane, no
    /// retirement ledger and no pending harness switch refers to.
    pub(crate) fn close_orphaned_agent_processes(&mut self, instances: &[Instance]) -> Result<()> {
        let mut owned: BTreeSet<&str> = self
            .panes
            .values()
            .map(|pane| pane.stored.instance_key.as_str())
            .collect();
        for agent in self.agents.values() {
            owned.extend(
                agent
                    .state
                    .retired_processes
                    .iter()
                    .map(|retired| retired.instance_key.as_str()),
            );
            owned.extend(
                agent
                    .state
                    .harness_switch
                    .iter()
                    .map(|switch| switch.candidate_instance_key.as_str()),
            );
        }
        let orphans: Vec<String> = instances
            .iter()
            .filter(|instance| {
                instance.spawn.kind == ProcessKind::PipesAcp
                    && !instance.closed
                    && !owned.contains(instance.spawn.instance_key.as_str())
            })
            .map(|instance| instance.instance_id.clone())
            .collect();
        for instance_id in orphans {
            self.pty_request(Request::Close { instance_id })?;
        }
        Ok(())
    }
}
