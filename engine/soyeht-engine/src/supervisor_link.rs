//! The engine's link to the process supervisor: when a lost link is retried, and what
//! becomes of a pane the supervisor refuses to start again.
//!
//! Reconnecting runs under the Core lock (ADR 0079), so a supervisor that cannot be reached
//! or used is retried with a growing delay instead of on every tick, and one pane that cannot
//! be restarted never keeps the whole link from being established.

use crate::{
    core::{Core, Registry},
    errors::{self, Error, Result},
    streak::Streak,
    supervisor_handle::Supervisor,
};
use soyeht_platform::{
    log::{self, Failure as _, Line},
    process::detached::spawn_service,
};
use soyeht_protocol::{
    PaneState,
    profile::{PTYD_LOG, PTYD_SOCKET},
};
use soyeht_ptyd::{client::Client, wire_kind};
use std::{collections::BTreeMap, io, path::PathBuf};

/// Delay after the first failed attempt is twice this; the delay doubles up to the cap.
const RETRY_BASE_MS: i64 = 100;
const RETRY_CAP_MS: i64 = 2_000;
/// How long a link must stay up before the next loss is treated as a new problem. A link
/// that is lost sooner counts as another failed attempt, so a supervisor that accepts the
/// hello and then drops is not reconnected without delay.
const STABLE_MS: i64 = 10_000;

/// How long after starting the supervisor the engine leaves it alone before it may start it
/// again: the time a supervisor takes to listen on a busy host.
const START_GRACE_MS: i64 = 15_000;
/// How long after a probe that found a supervisor (answering, or there and refusing) the engine
/// leaves it alone before it probes again, so a hung supervisor does not cost a probe on every
/// retry.
const PROBE_GRACE_MS: i64 = 5_000;
/// The background job that starts the supervisor, so there is one at a time.
const START_JOB: &str = "supervisor.start";
/// The background job that connects to the supervisor, so there is one at a time.
const CONNECT_JOB: &str = "supervisor.connect";

/// Why a pane that could not be restarted says it stopped; it names no path or command.
const NOT_RESTARTED: &str = "The pane's working folder or command is missing";

/// What the tick does about the supervisor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Link {
    Up,
    /// There is no link; the caller skips the work that needs one. The failure was logged
    /// here, once per change of kind and at growing intervals, never by the tick.
    Down,
}

pub(crate) struct Retry {
    streak: Streak,
    due_ms: i64,
    /// When the current link was established, until it has been up for [`STABLE_MS`].
    connected_ms: Option<i64>,
    /// The supervisor's program, when this engine is to start the supervisor again after it is
    /// lost. Whoever started the engine says so: a launch agent that keeps the supervisor alive
    /// does not, and then the engine only reconnects.
    program: Option<PathBuf>,
    /// The earliest time the program may be started or the supervisor probed again.
    start_due_ms: i64,
    /// The consecutive starts that failed, so a program that is missing is logged once.
    start_streak: Streak,
    /// The consecutive failures of each action the engine takes on the supervisor without
    /// being able to return the error (see [`Core::settle_supervisor_call`]), by event name.
    call_failures: BTreeMap<&'static str, Streak>,
    /// The panes whose process failed to start on a link that stays up, and when each is tried
    /// again (see `spawn_retry`).
    pub(crate) spawn_retries: crate::spawn_retry::SpawnRetries,
    /// The processes whose close failed on a link that stays up (see `spawn_retry`).
    pub(crate) close_retries: crate::spawn_retry::CloseRetries,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            streak: Streak::retrying(RETRY_BASE_MS, RETRY_CAP_MS),
            due_ms: 0,
            connected_ms: None,
            program: None,
            start_due_ms: 0,
            start_streak: Streak::once(),
            call_failures: BTreeMap::new(),
            spawn_retries: Default::default(),
            close_retries: Default::default(),
        }
    }
}

/// What the start of the supervisor found.
enum Start {
    /// The supervisor answers, so nothing was started.
    Answers,
    /// Nothing listens: the program was started.
    Started,
    /// Not started, for a reason the supervisor's own state explains.
    Left,
}

/// Whether a failed connection says that no supervisor listens, as opposed to one that is there
/// and refuses or does not answer.
fn supervisor_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

impl Core {
    /// Tells the engine to start the supervisor again from `program` when it cannot reach it.
    pub fn restart_supervisor_with(&mut self, program: PathBuf) {
        self.supervisor_retry.program = Some(program);
    }

    /// Starts the supervisor when the engine was told how and none listens. The check and the
    /// start run off the lock; one start at a time, and none within [`START_GRACE_MS`] of the
    /// last, so a supervisor that is still coming up is not started twice.
    fn start_supervisor(&mut self, now: i64) {
        let Some(program) = self.supervisor_retry.program.clone() else {
            return;
        };
        if now < self.supervisor_retry.start_due_ms {
            return;
        }
        let directory = self.directory.clone();
        let started = self.spawn_background(
            START_JOB.into(),
            move || {
                let reached = soyeht_platform::local::connect(&directory.join(PTYD_SOCKET));
                if reached.is_ok() {
                    return Ok(Start::Answers);
                }
                if reached.is_err_and(|error| !supervisor_absent(&error)) {
                    return Ok(Start::Left);
                }
                spawn_service(&program, &directory, PTYD_LOG, &[])?;
                Ok(Start::Started)
            },
            move |core, result| {
                let due = match result {
                    Ok(Start::Started) => {
                        core.supervisor_retry.start_streak.succeed();
                        Line::new("engine.supervisor_started").emit();
                        START_GRACE_MS
                    }
                    Ok(Start::Answers | Start::Left) => PROBE_GRACE_MS,
                    Err(error) => {
                        core.start_failed(&error);
                        START_GRACE_MS
                    }
                };
                core.supervisor_retry.start_due_ms = core.now_ms().saturating_add(due);
                Ok(())
            },
        );
        if let Err(error) = started {
            self.start_failed(&error);
        }
    }

    /// Logs a failed start of the supervisor the first time and whenever its kind changes.
    fn start_failed(&mut self, error: &Error) {
        if self
            .supervisor_retry
            .start_streak
            .fail(&error.failure_kind())
        {
            log::failure("engine.supervisor_start_failed", error);
        }
    }

    /// Connects to the supervisor when there is no link and the retry delay has passed. A
    /// tick that falls inside the delay does not touch the supervisor. A failed attempt is
    /// logged by the retry streak and reported as [`Link::Down`], not as an error, so the
    /// tick's own failure log cannot alternate with it.
    pub(crate) fn ensure_supervisor(&mut self) -> Link {
        let now = self.now_ms();
        if self.ptyd.as_ref().is_some_and(Supervisor::is_broken) {
            // A call left the stream in an unknown state: it is not used again, whatever the
            // jobs that failed on it have yet to settle.
            self.lose_supervisor_link();
        }
        if self.ptyd.is_some() {
            self.settle_link(now);
            self.keep_watch();
            return Link::Up;
        }
        if let Some(since) = self.supervisor_retry.connected_ms.take() {
            // The link was up and is gone.
            if now - since >= STABLE_MS {
                self.supervisor_retry.streak.succeed();
            } else {
                self.supervisor_retry.streak.fail("link_lost");
                self.supervisor_retry.due_ms =
                    now.saturating_add(self.supervisor_retry.streak.next_delay_ms());
            }
        }
        if now < self.supervisor_retry.due_ms {
            return Link::Down;
        }
        self.start_connect();
        // The connection is made off the lock and joined by a later tick, unless a test runs
        // its jobs in the caller.
        if self.ptyd.is_some() {
            Link::Up
        } else {
            Link::Down
        }
    }

    /// Starts the job that connects to the supervisor, unless one is running. The connection
    /// waits for the supervisor's welcome for up to five seconds, so it never runs under the
    /// lock; the registry is reconciled under it when the job is applied.
    fn start_connect(&mut self) {
        let socket = self.directory.join(PTYD_SOCKET);
        let generation = self.generation;
        // On a thread of its own: connecting to a socket of the local system can wait for as
        // long as the supervisor does not accept (a full backlog), and a shared worker must not
        // wait with it. One attempt runs at a time, so at most one such thread exists.
        self.background.run_alone(CONNECT_JOB);
        let started = self.spawn_background(
            CONNECT_JOB.into(),
            move || {
                let client = Client::connect(&socket, generation)
                    .map_err(|_| errors::ptyd_unavailable("Supervisor unavailable"))?;
                // The registry is read here, with the link not yet the engine's, so the
                // reconciliation under the lock sends the supervisor nothing it waits for.
                let supervisor = Supervisor::new(Box::new(client));
                let registry = Registry::read(&supervisor)?;
                Ok((supervisor, registry))
            },
            move |core, connected| {
                core.finish_connect(connected);
                Ok(())
            },
        );
        if let Err(error) = started {
            self.connect_failed(&error);
        }
    }

    /// Joins the connection a job made: reconciles the registry with the panes, or records the
    /// failure by the retry streak and starts the supervisor when the engine was told how.
    fn finish_connect(&mut self, connected: Result<(Supervisor, Registry)>) {
        let now = self.now_ms();
        match connected.and_then(|(link, registry)| self.adopt_registry(link, registry)) {
            Ok(()) => {
                // The reconciliation that made the link starts every pane that has no process.
                self.supervisor_retry.spawn_retries.clear();
                self.supervisor_retry.close_retries.clear();
                self.supervisor_retry.connected_ms = Some(now);
            }
            Err(error) => self.connect_failed(&error),
        }
    }

    fn connect_failed(&mut self, error: &Error) {
        let now = self.now_ms();
        if self.supervisor_retry.streak.fail(&error.failure_kind()) {
            log::failure("engine.supervisor_unavailable", error);
        }
        self.supervisor_retry.due_ms =
            now.saturating_add(self.supervisor_retry.streak.next_delay_ms());
        self.start_supervisor(now);
    }

    /// Ends the failure streak once the link has stayed up for [`STABLE_MS`].
    fn settle_link(&mut self, now: i64) {
        if let Some(since) = self.supervisor_retry.connected_ms
            && now - since >= STABLE_MS
        {
            self.supervisor_retry.connected_ms = None;
            self.supervisor_retry.streak.succeed();
        }
    }

    /// Whether `error` is the supervisor answering a spawn with a refusal that repeating cannot
    /// fix, on a link that is still up: the folder or the command is missing or not allowed.
    /// Every other failure, including a spawn that failed for lack of a resource or time, and
    /// a lost link, may pass and is not proof the pane cannot be restored.
    pub(crate) fn refused_by_supervisor(&self, error: &Error) -> bool {
        self.ptyd.is_some()
            && matches!(
                error,
                Error::Supervisor(soyeht_ptyd::Error::Remote { kind, .. })
                    if kind == wire_kind::SPAWN_REFUSED
            )
    }

    /// Settles an action on the supervisor whose caller has no error to return: a failed one
    /// is logged under `event` with its kind (once per change of kind, then at growing
    /// intervals), and the link is dropped only when the error says it is gone
    /// ([`errors::supervisor_link_lost`]). A refusal, a failure the supervisor reported or a
    /// storage error leaves a working link alone, so no pane is reconciled again.
    pub(crate) fn settle_supervisor_call(&mut self, event: &'static str, result: Result<()>) {
        let streak = self
            .supervisor_retry
            .call_failures
            .entry(event)
            .or_insert_with(|| Streak::retrying(RETRY_BASE_MS, RETRY_CAP_MS));
        let Err(error) = result else {
            streak.succeed();
            return;
        };
        if streak.fail(&error.failure_kind()) {
            log::failure(event, &error);
        }
        if errors::supervisor_link_lost(&error) && self.ptyd.is_some() {
            self.lose_supervisor_link();
        }
    }

    /// Handles the failure of one pane's recovery. When the supervisor refused to spawn it
    /// (see [`Core::refused_by_supervisor`]), the pane is stopped and the other panes are
    /// recovered; any other failure still fails the whole reconciliation, which is retried.
    pub(crate) fn stop_unrestartable_pane(&mut self, id: &str, error: Error) -> Result<()> {
        if let Some(reason) = error.unacknowledged_reason().map(str::to_owned) {
            log::failure("engine.pane_not_restarted", &error);
            return self.stop_pane(id, &reason);
        }
        if !self.refused_by_supervisor(&error) {
            return Err(error);
        }
        log::failure("engine.pane_not_restarted", &error);
        self.stop_pane(id, NOT_RESTARTED)
    }

    /// Stops a pane that has no process and will not get one, with `reason` as the phrase the
    /// clients show beside its dead state.
    pub(crate) fn stop_pane(&mut self, id: &str, reason: &str) -> Result<()> {
        if self.agents.contains_key(id) {
            return self.fail_agent(id, reason);
        }
        let mut stopped = self.panes[id].stored.clone();
        stopped.pane.pid = None;
        stopped.instance_id = None;
        stopped.supervisor_id = None;
        stopped.pane.state = PaneState::Dead;
        stopped.pane.phrase = reason.into();
        self.save_runtime(id, stopped)
    }
}

#[cfg(test)]
pub(crate) mod tests;
