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
use soyeht_ptyd::wire::Request;

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
        let closed = match instance {
            Some(instance_id) => self.pty_request(Request::Close { instance_id }).map(drop),
            None => Ok(()),
        };
        self.settle_supervisor_call("engine.role_close_failed", closed);
        // A lost link is rebuilt by the tick, which reconciles every pane.
        if self.ptyd.is_some() {
            // The recovery gives the pane a new identity and then spawns it, so a failed
            // spawn leaves a pane without a process: it is settled as a spawn (retried or
            // stopped), not only logged.
            let recovered = self.recover_missing_agent(pane);
            self.settle_spawn(pane, recovered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::{Running, state::Phase},
        model::{RunningPane, StoredPane},
        panes::persist_new_pane,
        ports::{StorePort, SupervisorPort, testing::FixedClock},
        screen::Screen,
        supervisor_link::tests::{events_named, short_directory},
    };
    use soyeht_protocol::{Pane, PaneMode, Workspace, event};
    use soyeht_ptyd::{
        Error as PtydError,
        wire::{Instance, Response},
        wire_kind,
    };
    use std::{collections::VecDeque, sync::Arc};

    /// A supervisor that fails the next spawns with the kinds it is given, starts the one
    /// after them, and answers everything else with `Ok`.
    struct Spawning {
        failures: VecDeque<&'static str>,
        spawns: usize,
    }

    impl SupervisorPort for Spawning {
        fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
            match request {
                Request::Spawn(config) => {
                    self.spawns += 1;
                    if let Some(kind) = self.failures.pop_front() {
                        return Err(PtydError::Remote {
                            kind: kind.into(),
                            message: "x".into(),
                        });
                    }
                    Ok(Response::Instance(Instance {
                        instance_id: format!("instance-{}", self.spawns),
                        supervisor_id: "spawning".into(),
                        spawn: config,
                        pid: 7,
                        closed: false,
                        exit_code: None,
                    }))
                }
                _ => Ok(Response::Ok),
            }
        }

        fn supervisor_id(&self) -> &str {
            "spawning"
        }

        fn supervisor_generation(&self) -> u32 {
            1
        }
    }

    /// A core with an agent pane that has a process, linked to a supervisor that fails the
    /// next spawns with `failures`; the clock is the test's.
    fn core_with_agent(failures: &[&'static str]) -> (Core, std::path::PathBuf) {
        let directory = short_directory();
        let state = crate::test_panes::ready_agent_state();
        let cwd = directory.to_string_lossy().into_owned();
        let pane = Pane {
            record: true,
            agent: Some(state.public(0)),
            ..crate::test_panes::pane("agent", "workspace", PaneMode::Agent, &cwd)
        };
        let saved = StoredPane {
            pane,
            closed: false,
            instance_key: "key".into(),
            instance_id: Some("instance-0".into()),
            supervisor_id: Some("spawning".into()),
            sequence_base: 0,
        };
        let workspace = Workspace {
            workspace_id: "workspace".into(),
            name: "W".into(),
            path: cwd,
            layout: json!({}),
        };
        let mut core = Core::open(&directory).unwrap();
        StorePort::mutate(&mut core.store, "test", "seed", "seed", |change| {
            change.insert_workspace(&workspace)?;
            persist_new_pane(change, &saved, Some(&state))?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
        let screen = Screen::open(&directory.join("sessions/agent"), true).unwrap();
        core.panes.insert(
            "agent".into(),
            RunningPane {
                stored: saved,
                screen,
            },
        );
        core.agents.insert("agent".into(), Running::new(state));
        core.clock = Arc::new(FixedClock(1_000_000));
        core.ptyd = Some(Box::new(Spawning {
            failures: failures.iter().copied().collect(),
            spawns: 0,
        }));
        (core, directory)
    }

    fn set_clock(core: &mut Core, ms: i64) {
        core.clock = Arc::new(FixedClock(ms));
    }

    #[test]
    fn a_role_change_whose_respawn_fails_is_retried_and_the_pane_comes_up() {
        let (mut core, _directory) = core_with_agent(&[wire_kind::SPAWN_FAILED]);
        core.restart_after_role_change("agent");

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

    #[test]
    fn a_role_change_whose_respawn_is_refused_stops_the_agent() {
        let (mut core, _directory) = core_with_agent(&[wire_kind::SPAWN_REFUSED]);
        core.restart_after_role_change("agent");

        assert!(core.ptyd.is_some());
        assert_eq!(core.panes["agent"].stored.pane.state, PaneState::Dead);
        assert_eq!(core.supervisor_retry.spawn_retries.attempts("agent"), 0);
        assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
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
}
