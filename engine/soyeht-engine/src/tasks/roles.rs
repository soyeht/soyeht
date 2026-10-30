//! Durable agent role assignment.

use super::api::parameters;
use crate::{
    agents::{Changes, state::Phase},
    core::Core,
    errors::{self, Result},
    outbox::CloseReason,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{AgentPaneRole, PaneRoleSetParams, PaneState, PromptOrigin, Role, method};

const LEAD_PROMPT: &str = "Coordinate; do not implement. Create small, self-contained tasks with how to verify. Never depend on another task's memory. Dispatch only when no free pane is available to claim.";

impl Core {
    pub(crate) fn pane_role_set(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        let params: PaneRoleSetParams = parameters(raw)?;
        let actor = self.task_actor(connection)?;
        if !matches!(actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Err(errors::forbidden_role(
                "Only a person may assign an agent role",
            ));
        }
        if self
            .store
            .active_task_for_pane(&params.pane_id)
            .map_err(errors::storage)?
            .is_some()
        {
            return Err(errors::pane_busy(
                "An agent role cannot change while the pane owns a task",
            ));
        }
        let mut state = self.agent_state(&params.pane_id)?;
        if state.harness_switch.is_some() {
            return Err(errors::pane_busy("Harness switch is still pending"));
        }
        let saved = self.panes[&params.pane_id].stored.clone();
        if saved.pane.state != PaneState::Idle
            || state.phase != Phase::Ready
            || state.active.is_some()
            || !self
                .store
                .agent_open_requests(&params.pane_id)
                .map_err(errors::storage)?
                .is_empty()
        {
            return Err(errors::pane_busy(
                "An agent role can change only while the pane is idle",
            ));
        }
        let role = match params.role {
            AgentPaneRole::Worker => Role::Agent,
            AgentPaneRole::Chief => Role::Chief,
        };
        let changed = state.mcp_role != role;
        if changed && role == Role::Chief {
            state.enqueue(
                LEAD_PROMPT.into(),
                PromptOrigin::Automatic,
                self.timestamp(),
            )?;
        }
        state.mcp_role = role;
        let result = json!({"paneId":params.pane_id,"role":params.role});
        let mutation = self
            .commit_agent(
                saved,
                state,
                Changes::default(),
                Some((connection, method::PANE_ROLE_SET, raw)),
                result,
            )?
            .result;
        if changed {
            self.restart_after_role_change(&params.pane_id);
        }
        Ok(mutation)
    }

    fn restart_after_role_change(&mut self, pane: &str) {
        for client in self.clients.values().filter(|client| {
            client
                .actor
                .as_ref()
                .and_then(|actor| actor.pane_id.as_deref())
                == Some(pane)
        }) {
            client.outbox.close(CloseReason::RoleChanged);
        }
        let instance = self
            .panes
            .get(pane)
            .and_then(|saved| saved.stored.instance_id.clone());
        if let Some(instance_id) = &instance {
            // A close that fails is logged and tried again by the tick; the new process starts
            // after it has been sent, not beside it.
            self.close_process(instance_id);
        }
        // A lost link is rebuilt by the tick, which reconciles every pane.
        if self.ptyd.is_some() {
            // The recovery gives the pane a new identity and then spawns it, so a failed
            // spawn leaves a pane without a process: it is settled as a spawn (retried or
            // stopped), not only logged.
            let recovered = self.recover_missing_agent_after(pane, instance.as_deref());
            self.spawn_begun(pane, recovered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::state::Phase, ports::testing::FixedClock, supervisor_link::tests::events_named,
        test_panes::core_with_agent,
    };
    use soyeht_protocol::event;
    use soyeht_ptyd::wire_kind;
    use std::sync::Arc;

    fn set_clock(core: &mut Core, ms: i64) {
        core.clock = Arc::new(FixedClock(ms));
    }

    /// The two ways an agent's process is respawned after it was lost: the role changes
    /// (its process is closed and recovered) and the drain finds it missing.
    const RESPAWNS: [fn(&mut Core); 2] = [
        |core| core.restart_after_role_change("agent"),
        |core| core.drain_panes().unwrap(),
    ];

    #[test]
    fn a_respawn_that_fails_is_retried_and_the_pane_comes_up() {
        for respawn in RESPAWNS {
            let (mut core, _directory) = core_with_agent(&[wire_kind::SPAWN_FAILED]);
            respawn(&mut core);

            assert!(core.ptyd.is_some());
            assert!(core.panes["agent"].stored.instance_id.is_none());
            assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 1);

            set_clock(&mut core, 1_100_000);
            core.advance_spawn_retries().unwrap();
            assert!(core.panes["agent"].stored.instance_id.is_some());
            assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 0);
            assert!(core.ptyd.is_some());
            assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
        }
    }

    #[test]
    fn a_respawn_that_is_refused_stops_the_agent() {
        for respawn in RESPAWNS {
            let (mut core, _directory) = core_with_agent(&[wire_kind::SPAWN_REFUSED]);
            respawn(&mut core);

            assert!(core.ptyd.is_some());
            assert_eq!(core.panes["agent"].stored.pane.state, PaneState::Dead);
            assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 0);
            assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
        }
    }

    #[test]
    fn an_agent_faulted_meanwhile_is_not_started_again() {
        let (mut core, _directory) = core_with_agent(&[wire_kind::SPAWN_FAILED]);
        core.restart_after_role_change("agent");
        assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 1);
        core.agents.get_mut("agent").unwrap().state.phase = Phase::Faulted;

        set_clock(&mut core, 1_100_000);
        core.advance_spawn_retries().unwrap();
        assert!(core.panes["agent"].stored.instance_id.is_none());
        assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 0);
    }

    #[test]
    fn a_terminal_the_supervisor_cannot_close_does_not_keep_the_agent_from_recovering() {
        use crate::{
            agents::terminals::Terminal,
            test_panes::{Spawning, pty_spawn},
        };
        let (mut core, _directory) = core_with_agent(&[]);
        // A terminal of the agent that the supervisor will not close: its close is tried again by
        // the tick, and the recovery goes on.
        core.agents
            .get_mut("agent")
            .unwrap()
            .state
            .terminals
            .insert(
                "terminal".into(),
                Terminal {
                    spawn: pty_spawn("terminal-key"),
                    instance_id: Some("child".into()),
                    supervisor_id: Some("spawning".into()),
                    bytes: String::new(),
                    from: 0,
                    next: 0,
                    limit: 1,
                    truncated: false,
                    exit_code: None,
                    message_seq: None,
                },
            );
        core.link_supervisor(Box::new(Spawning {
            failures: Default::default(),
            spawns: 0,
            close_failure: Some(wire_kind::OPERATION_FAILED),
        }));
        // The drain finds the process missing and the agent gets a new one.
        core.drain_panes().unwrap();
        assert!(core.ptyd.is_some());
        assert_eq!(
            core.panes["agent"].stored.instance_id.as_deref(),
            Some("instance-1")
        );
        assert_ne!(core.panes["agent"].stored.pane.state, PaneState::Dead);
        assert_ne!(core.agents["agent"].state.phase, Phase::Faulted);
        assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 0);
        // The ledger holds the terminal's process until the supervisor has closed it.
        assert!(
            core.agents["agent"]
                .state
                .retired_processes
                .iter()
                .any(|retired| retired.instance_id == "child")
        );
        assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
    }

    #[test]
    fn a_recovery_that_failed_before_the_pane_lost_its_process_is_not_retried_as_a_spawn() {
        let (mut core, _directory) = core_with_agent(&[]);
        core.settle_spawn("agent", Err(crate::errors::internal("storage")));

        assert!(core.ptyd.is_some());
        assert!(core.panes["agent"].stored.instance_id.is_some());
        assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 0);
        assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
    }

    #[test]
    fn the_replacing_process_starts_after_the_close_of_the_one_it_replaces() {
        use crate::test_panes::Answering;
        use soyeht_ptyd::wire::{Instance, Request, Response};
        use std::sync::Mutex;
        let (mut core, _directory) = core_with_agent(&[]);
        core.background = crate::offload::Background::default();
        let order = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&order);
        core.link_supervisor(Box::new(Answering(move |request| {
            Ok(match request {
                Request::Close { .. } => {
                    // The close takes its time: the spawn must not overtake it.
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    record.lock().unwrap().push("close");
                    Response::Ok
                }
                Request::Spawn(config) => {
                    record.lock().unwrap().push("spawn");
                    Response::Instance(Instance {
                        instance_id: "instance-1".into(),
                        supervisor_id: "answering".into(),
                        spawn: config,
                        pid: 7,
                        closed: false,
                        exit_code: None,
                    })
                }
                _ => Response::Ok,
            })
        })));

        core.restart_after_role_change("agent");
        crate::test_panes::apply_until_idle(&mut core);

        assert_eq!(*order.lock().unwrap(), ["close", "spawn"]);
        assert_eq!(
            core.panes["agent"].stored.instance_id.as_deref(),
            Some("instance-1")
        );
    }
}
