//! Idle agents cost nothing. An agent that is ready, has no turn, no queued
//! work, no operation and no open request needs nothing from the engine until
//! its state is committed or an open request expires. The tick still reads
//! the agent's output, at the idle pace of `read_ahead` (ADR 0118), but it
//! skips the whole advance pass: no SQL, no state clones and no deadline checks.
//!
//! The rule is event driven. `Idle::touch` runs on every committed change of
//! the agent's state, and a pass settles the agent only after it changed
//! nothing and the durable inputs it reads (open requests, outbox) were empty.

use super::{
    Running,
    state::{Phase, State},
};
use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use std::collections::BTreeSet;

/// What the engine remembers between ticks so an idle agent is not re-read.
#[derive(Default)]
pub struct Idle {
    /// Counts committed changes of this agent's state.
    revision: u64,
    /// The agent needs no advance pass before this time, unless it changes.
    quiet_until_ms: Option<i64>,
    /// The last output position confirmed to the supervisor.
    acknowledged: Option<(String, u64)>,
    /// Supervisor processes already told to close; a close is sent once.
    closed: BTreeSet<String>,
}

impl Idle {
    /// The agent's state changed: it must be advanced again.
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
        self.quiet_until_ms = None;
    }

    /// True once per distinct position of one process. Acknowledging again
    /// changes nothing for the supervisor.
    pub(crate) fn needs_acknowledgement(&mut self, instance_id: &str, seq: u64) -> bool {
        let position = (instance_id.to_owned(), seq);
        if self.acknowledged.as_ref() == Some(&position) {
            return false;
        }
        self.acknowledged = Some(position);
        true
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn already_closed(&self, instance_id: &str) -> bool {
        self.closed.contains(instance_id)
    }

    pub(crate) fn mark_closed(&mut self, instance_id: String) {
        self.closed.insert(instance_id);
    }
}

/// Counts clones of the agent state in tests, so a tick that copies it is
/// caught. It is zero-sized and does nothing outside tests.
#[derive(Debug, Default)]
pub struct CloneProbe;

impl Clone for CloneProbe {
    fn clone(&self) -> Self {
        #[cfg(test)]
        tests::CLONES.with(|count| count.set(count.get() + 1));
        Self
    }
}

/// Whether the state itself leaves the advance pass nothing to do. Open
/// requests and the outbox are durable and are checked separately.
fn has_nothing_to_advance(state: &State) -> bool {
    state.phase == Phase::Ready
        && state.active.is_none()
        && state.operation.is_none()
        && state.requested_options.is_empty()
        && state.cancel_at_ms.is_none()
        && state.probe_started_ms.is_none()
        && state.login_pane_id.is_none()
        && state.unconfirmed_login_pane_id.is_none()
        && state.terminals.is_empty()
        && state.client.pending.is_empty()
        && state.harness_switch.is_none()
}

impl Running {
    /// True while the agent is settled and no request of it can have expired.
    pub(crate) fn is_settled(&self, now_ms: i64) -> bool {
        self.idle.quiet_until_ms.is_some_and(|until| now_ms < until)
    }
}

impl Core {
    /// The agents that retired a process and still owe its close. With none,
    /// the tick copies no agent state and sends no request for cleanup.
    pub(crate) fn panes_with_retired_processes(&self) -> Vec<String> {
        self.agents
            .iter()
            .filter(|(_, agent)| !agent.state.retired_processes.is_empty())
            .map(|(pane, _)| pane.clone())
            .collect()
    }

    /// Settles the agent after an advance pass that committed nothing.
    /// `revision` is the agent's revision before the pass.
    pub(crate) fn settle_agent(&mut self, pane: &str, revision: u64) -> Result<()> {
        let Some(agent) = self.agents.get(pane) else {
            return Ok(());
        };
        if agent.idle.revision != revision || !has_nothing_to_advance(&agent.state) {
            return Ok(());
        }
        let open = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?;
        let queued_work = !agent.state.queue.is_empty() && !agent.state.queue_paused;
        if (queued_work && open.is_empty())
            || open
                .iter()
                .any(|r| r.method.starts_with("fs/") || r.method.starts_with("terminal/"))
            || !self
                .store
                .agent_outbox(pane)
                .map_err(errors::storage)?
                .is_empty()
        {
            return Ok(());
        }
        // An open question waits for a person; only its deadline needs a tick.
        let wake = open.iter().map(|r| r.expires_ms).min().unwrap_or(i64::MAX);
        errors::invariant(
            self.agents.get_mut(pane),
            "Agent runtime disappeared while settling",
        )?
        .idle
        .quiet_until_ms = Some(wake);
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        model::{RunningPane, StoredPane},
        ports::{
            SupervisorPort,
            testing::{EmptyPortScan, FixedClock},
        },
        screen::Screen,
    };
    use soyeht_protocol::Pane;
    use soyeht_ptyd::wire::{Request, Response};
    use std::{
        cell::Cell,
        fs,
        sync::{Arc, Mutex},
    };
    use uuid::Uuid;

    thread_local! {
        pub(in crate::agents) static CLONES: Cell<u64> = const { Cell::new(0) };
    }

    /// Answers like a supervisor whose process has produced no output, after `delay`, and
    /// counts the requests of each kind that arrived.
    fn quiet_supervisor(
        counters: [Arc<Mutex<u64>>; 3],
        delay: std::time::Duration,
    ) -> impl SupervisorPort {
        let [drains, acknowledgements, others] = counters;
        crate::test_panes::Answering(move |request| {
            std::thread::sleep(delay);
            let count = match request {
                Request::Drain { .. } => &drains,
                Request::Ack { .. } => &acknowledgements,
                _ => &others,
            };
            *count.lock().unwrap() += 1;
            Ok(match request {
                Request::Drain { from_seq, .. } => crate::test_panes::quiet_read(from_seq),
                _ => Response::Ok,
            })
        })
    }

    /// An agent that is ready for work, with nothing queued or open.
    pub(in crate::agents) fn ready_state() -> State {
        let launch = serde_json::json!({
            "id": "test", "name": "Test", "command": "/test", "args": [], "version": "1"
        });
        let mut state = State::new(
            serde_json::from_value(launch).unwrap(),
            "digest".into(),
            None,
        )
        .unwrap();
        state.phase = Phase::Ready;
        state.session_id = Some("session".into());
        state.requested_options.clear();
        state
    }

    fn idle_core(panes: usize) -> (Core, [Arc<Mutex<u64>>; 3], std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("soyeht-idle-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut core = Core::open(&root).unwrap();
        core.clock = Arc::new(FixedClock(0));
        core.web = crate::web::Service::new(Box::new(EmptyPortScan));
        // Reads end in the caller, so a tick takes the read it started.
        core.background = crate::offload::Background::inline();
        let counters: [Arc<Mutex<u64>>; 3] = Default::default();
        core.link_supervisor(Box::new(quiet_supervisor(
            counters.clone(),
            std::time::Duration::ZERO,
        )));
        for index in 0..panes {
            let id = format!("agent-{index}");
            let pane: Pane = serde_json::from_value(serde_json::json!({
                "paneId": id, "workspaceId": "workspace", "name": id,
                "cwd": root.display().to_string(), "mode": "agent", "state": "idle",
                "source": "protocol", "confidence": 1.0, "phrase": "Waiting",
                "generation": 1, "cols": 80, "rows": 24, "record": true, "pid": 1,
            }))
            .unwrap();
            let screen = Screen::open(&root.join("sessions").join(&id), true).unwrap();
            core.panes.insert(
                id.clone(),
                RunningPane {
                    stored: StoredPane {
                        pane,
                        closed: false,
                        instance_key: format!("key-{index}"),
                        instance_id: Some(format!("instance-{index}")),
                        supervisor_id: Some("idle-supervisor".into()),
                        sequence_base: 0,
                    },
                    screen,
                },
            );
            core.agents.insert(id, Running::new(ready_state()));
        }
        (core, counters, root)
    }

    fn total(counter: &Arc<Mutex<u64>>) -> u64 {
        *counter.lock().unwrap()
    }

    /// The regression guard for the idle engine. Ticks with settled agents
    /// run no SQL, copy no agent state, send no acknowledgement and read each
    /// agent only at the idle pace (`read_ahead`); and a change of an agent
    /// wakes it again.
    #[test]
    fn an_idle_tick_runs_no_sql_and_clones_no_agent_state() {
        let (mut core, [drains, acknowledgements, others], root) = idle_core(3);
        // The first ticks verify each agent is idle and settle it; the one after finds its
        // read empty and leaves it alone.
        for _ in 0..4 {
            core.poll_output().unwrap();
        }
        assert!(core.agents.values().all(|a| a.is_settled(core.now_ms())));
        let statements = soyeht_store::test_support::statements_started();
        let clones = CLONES.with(Cell::get);
        let (drained, acknowledged, other) =
            (total(&drains), total(&acknowledgements), total(&others));

        for _ in 0..50 {
            core.poll_output().unwrap();
        }

        assert_eq!(
            soyeht_store::test_support::statements_started() - statements,
            0,
            "an idle tick ran SQL"
        );
        assert_eq!(
            CLONES.with(Cell::get) - clones,
            0,
            "an idle tick cloned an agent state"
        );
        assert_eq!(
            total(&drains) - drained,
            0,
            "an idle agent was read on every tick"
        );
        assert_eq!(total(&acknowledgements) - acknowledged, 0);
        assert_eq!(total(&others) - other, 0);

        // When the idle pace has passed, each agent is read once and left alone again.
        core.clock = Arc::new(FixedClock(crate::read_ahead::IDLE_PACE_MS));
        for _ in 0..50 {
            core.poll_output().unwrap();
        }
        assert_eq!(total(&drains) - drained, 3, "one read per agent per pace");
        assert!(core.agents.values().all(|a| a.is_settled(core.now_ms())));

        let mut changed = core.agent_state("agent-1").unwrap();
        changed.queue_paused = true;
        core.replace_agent_state("agent-1", changed).unwrap();
        assert!(!core.agents["agent-1"].is_settled(core.now_ms()));
        assert!(core.agents["agent-0"].is_settled(core.now_ms()));
        // The agent that changed is read at once, not at the idle pace.
        let before = total(&drains);
        core.poll_output().unwrap();
        assert!(total(&drains) > before, "the changed agent was not read");
        crate::test_paths::remove_after(core, root);
    }

    fn retired(instance_id: &str, supervisor_id: &str) -> super::super::state::RetiredAgentProcess {
        super::super::state::RetiredAgentProcess {
            instance_key: format!("key-{instance_id}"),
            instance_id: instance_id.into(),
            supervisor_id: supervisor_id.into(),
        }
    }

    #[test]
    fn a_slow_supervisor_does_not_hold_the_tick_of_an_agent() {
        let (mut core, counters, root) = idle_core(2);
        let delay = std::time::Duration::from_millis(600);
        core.background = crate::offload::Background::default();
        core.link_supervisor(Box::new(quiet_supervisor(counters, delay)));
        core.agents
            .get_mut("agent-0")
            .unwrap()
            .state
            .retired_processes
            .push(retired("old", "idle-supervisor"));

        // Each tick starts the reads and the close of the retired process and goes on.
        for _ in 0..5 {
            let started = std::time::Instant::now();
            core.poll_output().unwrap();
            assert!(
                started.elapsed() < delay / 3,
                "the tick waited for the supervisor: {:?}",
                started.elapsed()
            );
        }
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn a_retired_process_leaves_the_ledger_once_it_is_closed_and_not_before() {
        use crate::test_panes::{Spawning, core_with_agent};
        let (mut core, directory) = core_with_agent(&[]);
        let ledger = |core: &Core| -> Vec<String> {
            core.agents["agent"]
                .state
                .retired_processes
                .iter()
                .map(|process| process.instance_id.clone())
                .collect()
        };
        core.agents
            .get_mut("agent")
            .unwrap()
            .state
            .retired_processes
            .extend([retired("mine", "spawning"), retired("gone", "dead")]);
        core.link_supervisor(Box::new(Spawning {
            failures: Default::default(),
            spawns: 0,
            close_failure: Some("refused"),
        }));

        // The supervisor refuses the close: the process stays, and the one of a supervisor that
        // is gone is forgotten.
        core.cleanup_retired_agent_processes().unwrap();
        assert_eq!(ledger(&core), ["mine"]);

        core.link_supervisor(Box::new(Spawning {
            failures: Default::default(),
            spawns: 0,
            close_failure: None,
        }));
        core.cleanup_retired_agent_processes().unwrap();
        assert!(ledger(&core).is_empty());
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn an_agent_with_queued_work_is_never_settled() {
        let (mut core, _, root) = idle_core(1);
        let mut state = core.agent_state("agent-0").unwrap();
        state
            .enqueue(
                "do work".into(),
                soyeht_protocol::PromptOrigin::Automatic,
                "fixed".into(),
            )
            .unwrap();
        state.draft_until_ms = i64::MAX;
        core.replace_agent_state("agent-0", state).unwrap();
        for _ in 0..3 {
            core.poll_output().unwrap();
        }
        assert!(!core.agents["agent-0"].is_settled(core.now_ms()));
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn cleanup_visits_only_agents_that_retired_a_process() {
        let (mut core, _, root) = idle_core(2);
        assert!(core.panes_with_retired_processes().is_empty());
        core.agents
            .get_mut("agent-1")
            .unwrap()
            .state
            .retired_processes
            .push(super::super::state::RetiredAgentProcess {
                instance_key: "old-key".into(),
                instance_id: "old-instance".into(),
                supervisor_id: "idle-supervisor".into(),
            });
        assert_eq!(core.panes_with_retired_processes(), ["agent-1"]);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn acknowledgement_and_close_are_sent_once_per_position() {
        let mut idle = Idle::default();
        assert!(idle.needs_acknowledgement("a", 4));
        assert!(!idle.needs_acknowledgement("a", 4));
        assert!(idle.needs_acknowledgement("a", 5));
        assert!(idle.needs_acknowledgement("b", 5));
        assert!(!idle.already_closed("a"));
        idle.mark_closed("a".into());
        assert!(idle.already_closed("a"));
    }
}
