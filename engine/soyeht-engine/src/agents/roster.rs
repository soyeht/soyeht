//! The state the engine keeps for its agents, apart from the store: the running agents, the
//! harness switches in flight, the catalogue epoch and the clients waiting on a native
//! operation. `Core` owns one `Agents` (`core.agents`); what a method needs of it alone is a
//! method here, and what also needs the store, the panes, the clock or the background jobs stays
//! an `impl Core` in the module of its job.

use super::{Running, state::State, switching::HarnessSwitchRuntime};
use crate::{
    errors::{self, Result},
    model::Waiter,
    tick_due::Next,
};
use std::collections::{BTreeMap, BTreeSet};

/// The most clients that wait on one native operation.
const NATIVE_WAITER_LIMIT: usize = 128;

#[derive(Default)]
pub struct Agents {
    /// The agent of each pane that has one, by pane id.
    pub running: BTreeMap<String, Running>,
    /// The harness switches that have started and not ended, by pane id (ADR 0126).
    pub(crate) switches: BTreeMap<String, HarnessSwitchRuntime>,
    /// Changes with each mutation of the harness catalogue, so a snapshot of it that a worker
    /// built meanwhile is not published stale.
    pub(crate) catalog_epoch: u64,
    /// The clients that wait for a native operation to complete, by the scope and the
    /// idempotency key of the operation.
    pub(crate) native_waiters: BTreeMap<(String, String), Vec<Waiter>>,
}

impl Agents {
    /// Starts to keep the agent of `pane`, whose state is `state`.
    pub(crate) fn adopt(&mut self, pane: String, state: State) {
        self.running.insert(pane, Running::new(state));
    }

    /// The catalogue changed: a snapshot built before this is stale.
    pub(crate) fn catalog_changed(&mut self) {
        self.catalog_epoch = self.catalog_epoch.wrapping_add(1);
    }

    /// The harnesses the running agents use.
    pub(crate) fn harness_ids(&self) -> BTreeSet<String> {
        self.running
            .values()
            .map(|agent| agent.state.launch.id.clone())
            .collect()
    }

    /// The agents that retired a process and still owe its close. With none, the tick copies no
    /// agent state and sends no request for cleanup.
    pub(crate) fn panes_with_retired_processes(&self) -> Vec<String> {
        self.running
            .iter()
            .filter(|(_, agent)| !agent.state.retired_processes.is_empty())
            .map(|(pane, _)| pane.clone())
            .collect()
    }

    /// When the tick must close the processes the agents retired: when the delay a refusal left
    /// ends, for each agent that owes a close.
    pub(crate) fn retired_processes_due(&self, next: &mut Next, now: i64) {
        for agent in self.running.values() {
            if !agent.state.retired_processes.is_empty() {
                next.at_ms(now, agent.idle.retired_close_due_ms());
            }
        }
    }

    /// Adds to `owned` the processes the agents hold besides their own: the ones they retired and
    /// the candidate of a harness switch.
    pub(crate) fn extend_owned_instances<'a>(&'a self, owned: &mut BTreeSet<&'a str>) {
        for agent in self.running.values() {
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
    }

    /// Whether a running harness marks `mode` as the one that approves everything.
    pub(crate) fn marks_full_access(&self, mode: &str, kind: &str) -> bool {
        self.running.values().any(|agent| {
            agent.state.options.mode().is_some_and(|option| {
                option
                    .options
                    .iter()
                    .any(|choice| choice.value == mode && choice.kind.as_deref() == Some(kind))
            })
        })
    }

    /// Makes `waiter` wait for the native operation of `scope` and `key`, once.
    pub(crate) fn wait_for_native(
        &mut self,
        scope: String,
        key: String,
        waiter: Waiter,
    ) -> Result<()> {
        let waiters = self.native_waiters.entry((scope, key)).or_default();
        if waiters.iter().any(|known| {
            known.connection == waiter.connection && known.request_id == waiter.request_id
        }) {
            return Ok(());
        }
        if waiters.len() >= NATIVE_WAITER_LIMIT {
            return Err(errors::pane_busy("Native operation waiter limit reached"));
        }
        waiters.push(waiter);
        Ok(())
    }

    /// The clients that wait for the native operation of `scope` and `key`, which no longer waits.
    pub(crate) fn take_native_waiters(&mut self, scope: &str, key: &str) -> Vec<Waiter> {
        self.native_waiters
            .remove(&(scope.to_owned(), key.to_owned()))
            .unwrap_or_default()
    }

    /// Forgets what the connection `id` waited for.
    pub(crate) fn forget_connection(&mut self, id: &str) {
        self.native_waiters.retain(|_, waiters| {
            waiters.retain(|waiter| waiter.connection != id);
            !waiters.is_empty()
        });
    }
}
