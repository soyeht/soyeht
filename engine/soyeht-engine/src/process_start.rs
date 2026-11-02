//! Starting a process off the Core lock (ADR 0126).
//!
//! A `Spawn` is a request to the supervisor, and the supervisor can take as long as the client's
//! timeout to answer it. The start of a process is therefore two steps with no lock between
//! them: the request goes to the supervisor through a [`Launch`], and the answer is adopted into
//! the pane, the inline terminal or the language server that asked for it, under the lock. A
//! request that answers a client (`pane.create`) carries the launch in an `Outcome::Offload`;
//! the engine's own starts (a recovery, a retry, a restart, a task) run it as a
//! `Core::spawn_background` job.
//!
//! What a pane is meanwhile is the state it already has when a start failed and waits for its
//! retry (`spawn_retry`): its identity is reserved and saved (key, generation, stream), and it
//! has no process. Nothing polls such a pane, and a pane that closes or is given another
//! identity in the meantime is noticed when the start ends ([`Core::finish_pane_start`]).
//!
//! The supervisor starts a process once per instance key, so a start that is asked again (the
//! link was replaced while it ran, the tick tries a pane again) gets the process the first one
//! made.

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Error, Result},
    offload::Offload,
    ports::StorePort,
    supervisor_handle::{DEADLINE, Supervisor, Turn},
};
use serde_json::Value;
use soyeht_ptyd::wire::{Instance, ProcessKind, Request, Response, Spawn};

/// How a start ended, as the code that asked for it sees it under the lock.
pub(crate) enum Started {
    /// The supervisor made the process.
    Process(Instance),
    /// The supervisor could not (a lost link is already settled).
    Failed(Error),
    /// The link the request went through is not the engine's any more: the answer, whatever it
    /// was, says nothing about the process on the link that replaced it.
    Stale,
}

/// The request for a process and its place in the order of the sends, taken under the lock.
/// [`Launch::send`] waits for the supervisor and must run off it.
pub(crate) struct Launch {
    config: Spawn,
    supervisor: Supervisor,
    turn: Turn,
}

impl Launch {
    /// The link the request goes through, to settle its answer by (see [`Core::started`]).
    pub(crate) fn link(&self) -> Supervisor {
        self.supervisor.clone()
    }

    /// Waits for the sends ahead of it, then asks the supervisor.
    pub(crate) fn send(self) -> Result<Instance> {
        self.turn.wait();
        match self.supervisor.call(Request::Spawn(self.config))? {
            Response::Instance(instance) => Ok(instance),
            _ => Err(errors::ptyd_unavailable("Invalid spawn response")),
        }
    }
}

impl Core {
    /// Takes a place for the start of `config`, after the sends still on their way to the
    /// processes `after` (a process this one replaces, which must not still hold its session or
    /// its folder when this one starts, ADR 0119). It fails when there is no link.
    pub(crate) fn launch(&mut self, config: Spawn, after: &[&str]) -> Result<Launch> {
        let supervisor = self.supervisor()?;
        let turn = self.turn(after);
        Ok(Launch {
            config,
            supervisor,
            turn,
        })
    }

    /// Takes the answer of a launch back under the lock.
    pub(crate) fn started(&mut self, link: &Supervisor, result: Result<Instance>) -> Started {
        let current = self.is_current_link(link);
        match self.settle_supervisor(link, result) {
            _ if !current => Started::Stale,
            Ok(instance) => Started::Process(instance),
            Err(error) => Started::Failed(error),
        }
    }

    /// Starts the process of `owner` (a pane, an inline terminal, a language server) as a job off
    /// the lock and gives the result to `done` under it. Nothing is sent when a start for `owner`
    /// is already on its way: what asked for that one settles it. An error is for a start that
    /// could not begin (no link); the supervisor's answer goes to `done`.
    pub(crate) fn start_process(
        &mut self,
        owner: &str,
        config: Spawn,
        after: &[&str],
        done: impl FnOnce(&mut Core, Started) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        let job = format!("start:{owner}");
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let launch = self.launch(config, after)?;
        let link = launch.link();
        self.spawn_prompt_background(
            job,
            move || launch.send(),
            move |core, result| {
                let started = core.started(&link, result);
                done(core, started)
            },
            // The tick applies it at once: whatever asked is waiting for its process.
            |_| true,
        )
    }

    /// Starts the process of pane `id`, which was given its identity (and saved it) before this
    /// call, and adopts it when the supervisor has made it. An error is for a start that could
    /// not begin, which the caller settles (`settle_spawn`); the supervisor's failure is settled
    /// when it comes.
    pub fn spawn(&mut self, id: &str) -> Result<()> {
        self.spawn_after(id, None)
    }

    /// [`Core::spawn`] for a process that replaces the process `after`: it starts once its
    /// close has been sent.
    pub(crate) fn spawn_after(&mut self, id: &str, after: Option<&str>) -> Result<()> {
        let config = self.pane_spawn_config(id)?;
        let key = config.instance_key.clone();
        let (owner, pane) = (format!("{id}:{key}"), id.to_owned());
        // The job is keyed by the identity too: a pane given another one while a start runs is
        // started under it at once, not when the first start has ended.
        self.start_process(&owner, config, after.as_slice(), move |core, started| {
            core.finish_pane_start(&pane, &key, started);
            Ok(())
        })
    }

    /// What the supervisor is asked to start for pane `id`: its agent's process, or its shell (or
    /// the harness a terminal pane runs).
    pub(crate) fn pane_spawn_config(&self, id: &str) -> Result<Spawn> {
        if self.agents.contains_key(id) {
            return Ok(self.agent_spawn_config(id));
        }
        let pane = &self.panes[id].stored;
        let launch: Option<Spawn> = match self.terminal_harnesses.get(id) {
            Some(session) => Some(session.spawn.clone()),
            None => StorePort::pane_launch(&self.store, id).map_err(errors::storage)?,
        };
        Ok(Spawn {
            instance_key: pane.instance_key.clone(),
            generation: pane.pane.generation,
            kind: ProcessKind::Pty,
            cwd: pane.pane.cwd.clone(),
            argv: launch
                .as_ref()
                .map(|l| l.argv.clone())
                .unwrap_or_else(soyeht_ptyd::shell::default_login_shell),
            env: launch.map(|l| l.env).unwrap_or_default(),
            cols: pane.pane.cols as u16,
            rows: pane.pane.rows as u16,
        })
    }

    /// Starts the process of the pane `id` that a request just created, and answers the request
    /// with `result` when the supervisor has made it (or failed to): the wait is the work of an
    /// `Outcome::Offload`, so it holds neither the lock nor the requests behind it on other
    /// connections. A start that cannot begin (no link) answers at once and leaves the pane to
    /// its retry.
    pub(crate) fn spawn_then_reply(&mut self, id: &str, result: Value) -> Result<Outcome> {
        let launch = self
            .pane_spawn_config(id)
            .and_then(|config| Ok((config.instance_key.clone(), self.launch(config, &[])?)));
        let (key, launch) = match launch {
            Ok(launch) => launch,
            Err(error) => {
                self.settle_spawn(id, Err(error));
                return Ok(Outcome::Reply(result));
            }
        };
        let (link, pane) = (launch.link(), id.to_owned());
        Ok(Outcome::Offload(Offload::new(
            "Supervisor spawn timed out",
            DEADLINE,
            move || launch.send(),
            move |core, sent| {
                let started = core.started(&link, sent);
                core.finish_pane_start(&pane, &key, started);
                Ok(Outcome::Reply(result))
            },
        )))
    }

    /// Settles a start of pane `id`'s process under the lock. A pane that closed, or got another
    /// identity, while it ran does not take the process: it is closed. A pane that still has no
    /// process and wants one is started again, with the identity it has now.
    pub(crate) fn finish_pane_start(&mut self, id: &str, key: &str, started: Started) {
        let (wanted, same) = self.panes.get(id).map_or((false, false), |pane| {
            let stored = &pane.stored;
            (
                !stored.closed && stored.instance_id.is_none(),
                stored.instance_key == key,
            )
        });
        let result = match started {
            Started::Process(instance) if wanted && same => self.adopt(id, &instance),
            Started::Process(instance) => {
                // Unless the reconciliation of a new link already adopted this very process.
                let adopted = self.panes.get(id).is_some_and(|pane| {
                    pane.stored.instance_id.as_deref() == Some(instance.instance_id.as_str())
                });
                if !adopted {
                    self.close_process(&instance.instance_id);
                }
                return self.start_again(id, wanted);
            }
            Started::Failed(error) if wanted && same => Err(error),
            Started::Failed(_) => return self.start_again(id, wanted),
            Started::Stale => {
                // The start of this identity has not ended as far as the job table knows, so it
                // is the tick that starts it again, on the link that replaced the one it used.
                if wanted && self.ptyd.is_some() {
                    self.schedule_spawn_retry(id);
                }
                return;
            }
        };
        self.settle_spawn(id, result);
    }

    /// Starts pane `id` again when it wants a process and the link is up.
    fn start_again(&mut self, id: &str, wanted: bool) {
        if wanted && self.ptyd.is_some() {
            let spawned = self.spawn(id);
            self.spawn_begun(id, spawned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        offload::Background,
        ports::SupervisorPort,
        supervisor_link::tests::core_with_unstarted_pane,
        test_panes::{Answering, apply_until_done},
    };
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    /// What the supervisor was asked, in order.
    type Asked = Arc<Mutex<Vec<String>>>;

    /// A supervisor that takes `delay` to answer a start or a close, and records both.
    fn slow_supervisor(delay: Duration, asked: &Asked) -> Box<dyn SupervisorPort> {
        let asked = Arc::clone(asked);
        Box::new(Answering(move |request| match request {
            Request::Spawn(config) => {
                asked
                    .lock()
                    .unwrap()
                    .push(format!("spawn {}", config.instance_key));
                std::thread::sleep(delay);
                Ok(Response::Instance(Instance {
                    instance_id: format!("process-of-{}", config.instance_key),
                    supervisor_id: "answering".into(),
                    spawn: config,
                    pid: 7,
                    closed: false,
                    exit_code: None,
                }))
            }
            Request::Close { instance_id } => {
                asked.lock().unwrap().push(format!("close {instance_id}"));
                std::thread::sleep(delay);
                Ok(Response::Ok)
            }
            _ => Ok(Response::Ok),
        }))
    }

    /// A core whose pane `p-pane` has no process, on a supervisor that takes `delay` to answer
    /// and jobs that run on the pool, as in the engine.
    fn slow_core(delay: Duration) -> (Core, std::path::PathBuf, Asked) {
        let (mut core, directory, _, _) = core_with_unstarted_pane(Vec::new());
        core.background = Background::default();
        let asked = Asked::default();
        core.link_supervisor(slow_supervisor(delay, &asked));
        (core, directory, asked)
    }

    const PANE: &str = "p-pane";
    const START: &str = "start:p-pane:p-key";

    fn asked(asked: &Asked) -> Vec<String> {
        asked.lock().unwrap().clone()
    }

    #[test]
    fn a_pane_is_started_off_the_lock_and_adopts_its_process_when_the_tick_applies_it() {
        let (mut core, directory, requests) = slow_core(Duration::from_secs(1));

        let began = Instant::now();
        core.spawn(PANE).unwrap();
        // A second start of the same pane while one is on its way asks for nothing more.
        core.spawn(PANE).unwrap();
        assert!(
            began.elapsed() < Duration::from_millis(300),
            "the start waited for the supervisor: {:?}",
            began.elapsed()
        );
        assert!(core.panes[PANE].stored.instance_id.is_none());

        apply_until_done(&mut core, START);
        assert_eq!(
            core.panes[PANE].stored.instance_id.as_deref(),
            Some("process-of-p-key")
        );
        assert_eq!(asked(&requests), ["spawn p-key"]);
        assert_eq!(core.supervisor_retry.spawn_retries.attempts(PANE), 0);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_pane_that_closes_while_its_process_starts_does_not_take_it() {
        let (mut core, directory, requests) = slow_core(Duration::from_millis(300));
        core.spawn(PANE).unwrap();
        core.panes.get_mut(PANE).unwrap().stored.closed = true;

        apply_until_done(&mut core, START);
        apply_until_done(&mut core, "close:process-of-p-key");
        assert!(core.panes[PANE].stored.instance_id.is_none());
        assert_eq!(asked(&requests), ["spawn p-key", "close process-of-p-key"]);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_pane_given_another_identity_while_it_starts_closes_the_old_process_and_starts_again() {
        let (mut core, directory, requests) = slow_core(Duration::from_millis(300));
        core.spawn(PANE).unwrap();
        core.panes.get_mut(PANE).unwrap().stored.instance_key = "q-key".into();

        apply_until_done(&mut core, START);
        // The new identity is started at once, beside the old start that has just ended.
        apply_until_done(&mut core, "start:p-pane:q-key");
        assert_eq!(
            core.panes[PANE].stored.instance_id.as_deref(),
            Some("process-of-q-key")
        );
        apply_until_done(&mut core, "close:process-of-p-key");
        let mut seen = asked(&requests);
        seen.sort();
        assert_eq!(
            seen,
            ["close process-of-p-key", "spawn p-key", "spawn q-key"]
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_start_answered_on_a_link_that_was_replaced_is_left_to_the_tick() {
        let (mut core, directory, _requests) = slow_core(Duration::from_millis(300));
        core.spawn(PANE).unwrap();
        // The link is replaced before the old one answers.
        let replacement = Asked::default();
        core.link_supervisor(slow_supervisor(Duration::ZERO, &replacement));

        apply_until_done(&mut core, START);
        assert!(core.panes[PANE].stored.instance_id.is_none());
        assert!(core.ptyd.is_some());
        assert_eq!(core.supervisor_retry.spawn_retries.attempts(PANE), 1);
        assert!(asked(&replacement).is_empty());
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_process_that_replaces_another_starts_after_the_close_of_the_one_it_replaces() {
        let (mut core, directory, requests) = slow_core(Duration::from_millis(300));
        let began = Instant::now();
        core.close_process("old-process");
        core.spawn_after(PANE, Some("old-process")).unwrap();
        assert!(
            began.elapsed() < Duration::from_millis(250),
            "the restart waited under the lock: {:?}",
            began.elapsed()
        );

        apply_until_done(&mut core, START);
        apply_until_done(&mut core, "close:old-process");
        assert_eq!(asked(&requests), ["close old-process", "spawn p-key"]);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_created_pane_answers_when_its_process_has_started_and_not_before() {
        let (mut core, directory, _requests) = slow_core(Duration::from_millis(200));
        let outcome = core
            .spawn_then_reply(PANE, serde_json::json!({"paneId":PANE}))
            .unwrap();
        let crate::dispatch::Outcome::Offload(offload) = outcome else {
            panic!("a pane with a process waits for it off the lock");
        };
        assert!(core.panes[PANE].stored.instance_id.is_none());

        let began = Instant::now();
        let reply = offload.run_inline(&mut core).unwrap();
        assert!(began.elapsed() >= Duration::from_millis(150));
        assert!(matches!(reply, crate::dispatch::Outcome::Reply(value) if value["paneId"] == PANE));
        assert_eq!(
            core.panes[PANE].stored.instance_id.as_deref(),
            Some("process-of-p-key")
        );
        crate::test_paths::remove_after(core, directory);
    }

    fn instance_named(id: &str) -> Instance {
        Instance {
            instance_id: id.into(),
            supervisor_id: "answering".into(),
            spawn: crate::test_panes::pty_spawn("p-key"),
            pid: 7,
            closed: false,
            exit_code: None,
        }
    }

    #[test]
    fn a_process_the_pane_already_adopted_is_not_closed_as_an_orphan() {
        let (mut core, directory, requests) = slow_core(Duration::ZERO);
        // The reconciliation of a new link adopted this very process before the start ended.
        core.panes.get_mut(PANE).unwrap().stored.instance_id = Some("adopted".into());

        core.finish_pane_start(PANE, "p-key", Started::Process(instance_named("adopted")));
        crate::test_panes::apply_until_idle(&mut core);
        assert!(asked(&requests).is_empty());

        // Another process than the one the pane holds is an orphan.
        core.finish_pane_start(PANE, "p-key", Started::Process(instance_named("stray")));
        crate::test_panes::apply_until_idle(&mut core);
        assert_eq!(asked(&requests), ["close stray"]);
        crate::test_paths::remove_after(core, directory);
    }
}
