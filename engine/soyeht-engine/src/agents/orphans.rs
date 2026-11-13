//! Agent and language server processes that nothing in the engine owns. The supervisor keeps an
//! instance until it is closed, so one that the engine forgot (a pane row
//! removed, a retirement ledger lost with an old build) would live, with its
//! adapter tree, until the supervisor restarts. Reconciliation closes them.

use crate::{core::Core, errors::Result};
use soyeht_ptyd::wire::{Instance, ProcessKind};
use std::collections::BTreeSet;

impl Core {
    /// Closes every open agent or language server process in `instances` that no pane, no
    /// retirement ledger, no pending harness switch and no language server refers to.
    pub(crate) fn close_orphaned_agent_processes(&mut self, instances: &[Instance]) -> Result<()> {
        let mut owned: BTreeSet<&str> = self
            .panes
            .values()
            .map(|pane| pane.stored.instance_key.as_str())
            .collect();
        self.agents.extend_owned_instances(&mut owned);
        // A language server owns the process of its current instance key: one that was restarted
        // (a trust that changed while the supervisor was away) left the old one behind.
        owned.extend(
            self.languages
                .values()
                .map(|server| server.instance_key.as_str()),
        );
        let orphans: Vec<String> = instances
            .iter()
            .filter(|instance| {
                matches!(
                    instance.spawn.kind,
                    ProcessKind::PipesAcp | ProcessKind::PipesLsp
                ) && !instance.closed
                    && !owned.contains(instance.spawn.instance_key.as_str())
            })
            .map(|instance| instance.instance_id.clone())
            .collect();
        for instance_id in orphans {
            // Best effort: one that stays open is closed again by the tick, or by the next link.
            self.close_process(&instance_id);
        }
        Ok(())
    }
}
