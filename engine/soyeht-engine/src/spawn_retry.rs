//! A pane whose process failed to start on a supervisor link that is still up.
//!
//! The pane has no process and, on this link, nothing else would start one: reconciliation
//! runs only when a link is made. So the tick tries again, paced by a [`Streak`] per pane, up
//! to [`MAX_ATTEMPTS`] failures in all. A spawn the supervisor refuses, or the last failed
//! attempt, stops the pane with a reason the clients show beside its dead state. A lost link
//! ends all of it: the reconciliation that follows starts every pane again.
//!
//! The same holds for the close of a pane's process (`CloseRetries`): a close the supervisor
//! failed on a working link is tried again by the tick, a bounded number of times, instead of
//! leaving the process running until a link is made.

use crate::{
    agents::state::Phase,
    core::Core,
    errors::{self, Result},
    streak::Streak,
};
use soyeht_platform::log::Line;
use soyeht_protocol::PaneState;
use soyeht_ptyd::wire::Request;
use std::collections::BTreeMap;

/// The delay after the first failed attempt is twice this; the delay doubles up to the cap.
const RETRY_BASE_MS: i64 = 250;
const RETRY_CAP_MS: i64 = 4_000;
/// Failed starts of one pane (the first included) after which the pane is stopped.
const MAX_ATTEMPTS: u32 = 5;
/// The event of a failed start in the log, once per change of kind and at growing intervals.
const SPAWN_FAILED_EVENT: &str = "engine.pane_spawn_failed";
/// Why a pane that could not be started says it stopped; it names no path or command.
const NOT_STARTED: &str = "The pane's process could not be started";

struct Pending {
    streak: Streak,
    due_ms: i64,
}

impl Pending {
    fn new(now: i64) -> Self {
        Self {
            streak: Streak::retrying(RETRY_BASE_MS, RETRY_CAP_MS),
            due_ms: now,
        }
    }

    /// Counts one more failed attempt and schedules the next. Returns false when the attempts
    /// are used up.
    fn failed(&mut self, now: i64) -> bool {
        self.streak.fail("failed");
        self.due_ms = now.saturating_add(self.streak.next_delay_ms());
        self.streak.attempts() < MAX_ATTEMPTS
    }
}

/// The processes whose close failed on a working link, by instance, and when each is tried
/// again.
#[derive(Default)]
pub(crate) struct CloseRetries(BTreeMap<String, Pending>);

impl CloseRetries {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    #[cfg(test)]
    pub(crate) fn attempts(&self, instance_id: &str) -> u32 {
        self.0
            .get(instance_id)
            .map_or(0, |pending| pending.streak.attempts())
    }
}

#[derive(Default)]
pub(crate) struct SpawnRetries(BTreeMap<String, Pending>);

impl SpawnRetries {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    #[cfg(test)]
    pub(crate) fn attempts(&self, id: &str) -> u32 {
        self.0
            .get(id)
            .map_or(0, |pending| pending.streak.attempts())
    }
}

impl Core {
    /// Settles the start of a pane's process that the caller cannot return. A spawn the
    /// supervisor refused stops the pane, as its reconciliation would (see
    /// [`Core::stop_unrestartable_pane`]). Another failure on a working link is logged and
    /// the pane is tried again by the tick; a lost link is left to the reconciliation.
    pub(crate) fn settle_spawn(&mut self, id: &str, result: Result<()>) {
        let error = match result {
            Ok(()) => {
                self.supervisor_retry.spawn_retries.0.remove(id);
                return self.settle_supervisor_call(SPAWN_FAILED_EVENT, Ok(()));
            }
            Err(error) => error,
        };
        if self.refused_by_supervisor(&error) {
            self.supervisor_retry.spawn_retries.0.remove(id);
            let stopped = self.stop_unrestartable_pane(id, error);
            return self.settle_supervisor_call(SPAWN_FAILED_EVENT, stopped);
        }
        // Only a pane with no process is waiting for one. A recovery that failed before the
        // pane lost its process (a storage error, a terminal that would not close) is
        // attempted again by whatever found the process missing, on its own pace, and must
        // not use up the attempts that stop a pane.
        let waiting = self
            .panes
            .get(id)
            .is_some_and(|pane| pane.stored.instance_id.is_none());
        let retryable = waiting && self.ptyd.is_some() && !errors::supervisor_link_lost(&error);
        self.settle_supervisor_call(SPAWN_FAILED_EVENT, Err(error));
        if retryable && self.ptyd.is_some() {
            self.schedule_spawn_retry(id);
        }
    }

    /// Counts one more failed start of `id`, and either schedules the next try or stops the
    /// pane.
    fn schedule_spawn_retry(&mut self, id: &str) {
        let now = self.now_ms();
        let pending = self
            .supervisor_retry
            .spawn_retries
            .0
            .entry(id.to_owned())
            .or_insert_with(|| Pending::new(now));
        if pending.failed(now) {
            return;
        }
        self.supervisor_retry.spawn_retries.0.remove(id);
        Line::new("engine.pane_not_started").emit();
        let stopped = self.stop_pane(id, NOT_STARTED);
        self.settle_supervisor_call("engine.pane_stop_failed", stopped);
    }

    /// Settles the close of the process `instance_id` of a closed pane, which the caller cannot
    /// return. A failure on a working link is logged and tried again by the tick; after
    /// [`MAX_ATTEMPTS`] failures the process is left to the reconciliation of the next link,
    /// which closes the processes of closed panes.
    pub(crate) fn settle_close(&mut self, instance_id: &str, result: Result<()>) {
        let retryable = matches!(&result, Err(error) if !errors::supervisor_link_lost(error));
        if result.is_ok() {
            self.supervisor_retry.close_retries.0.remove(instance_id);
        }
        self.settle_supervisor_call("engine.pane_close_failed", result);
        if !retryable || self.ptyd.is_none() {
            return;
        }
        let now = self.now_ms();
        let pending = self
            .supervisor_retry
            .close_retries
            .0
            .entry(instance_id.to_owned())
            .or_insert_with(|| Pending::new(now));
        if !pending.failed(now) {
            self.supervisor_retry.close_retries.0.remove(instance_id);
            Line::new("engine.pane_close_given_up").emit();
        }
    }

    /// The tick's part for closes: closes again each process whose retry is due.
    pub(crate) fn advance_close_retries(&mut self) -> Result<()> {
        let now = self.now_ms();
        let due: Vec<String> = self
            .supervisor_retry
            .close_retries
            .0
            .iter()
            .filter(|(_, pending)| pending.due_ms <= now)
            .map(|(instance_id, _)| instance_id.clone())
            .collect();
        for instance_id in due {
            self.close_process(&instance_id);
        }
        Ok(())
    }

    /// Closes the process `instance_id` of a closed pane and settles the result.
    pub(crate) fn close_process(&mut self, instance_id: &str) {
        let closed = self
            .pty_request(Request::Close {
                instance_id: instance_id.to_owned(),
            })
            .map(drop);
        self.settle_close(instance_id, closed);
    }

    /// The tick's part: starts again each pane whose retry is due. A pane that closed, or
    /// got a process meanwhile, is forgotten.
    pub(crate) fn advance_spawn_retries(&mut self) -> Result<()> {
        let now = self.now_ms();
        let due: Vec<String> = self
            .supervisor_retry
            .spawn_retries
            .0
            .iter()
            .filter(|(_, pending)| pending.due_ms <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            let startable = self.panes.get(&id).is_some_and(|pane| {
                !pane.stored.closed
                    && pane.stored.instance_id.is_none()
                    && pane.stored.pane.state != PaneState::Dead
            }) && self
                .agents
                .get(&id)
                .is_none_or(|agent| agent.state.phase != Phase::Faulted);
            if !startable {
                self.supervisor_retry.spawn_retries.0.remove(&id);
                continue;
            }
            let spawned = self.spawn(&id);
            self.settle_spawn(&id, spawned);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor_link::{
        Link,
        tests::{Manual, core_with_unstarted_pane, events_named, remote},
    };
    use crate::test_panes::pty_spawn;
    use soyeht_protocol::event;
    use soyeht_ptyd::{
        Error as PtydError,
        wire::{Instance, Response},
        wire_kind,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    };

    fn failed_start() -> soyeht_ptyd::Result<Response> {
        Err(PtydError::Remote {
            kind: wire_kind::SPAWN_FAILED.into(),
            message: "no pty".into(),
        })
    }

    fn started() -> [soyeht_ptyd::Result<Response>; 2] {
        let instance = Instance {
            instance_id: "instance".into(),
            supervisor_id: "scripted".into(),
            spawn: pty_spawn("p-key"),
            pid: 7,
            closed: false,
            exit_code: None,
        };
        // The spawn, then the resize that adopts it.
        [Ok(Response::Instance(instance)), Ok(Response::Ok)]
    }

    /// A core whose clock the test moves, with its pane's first start already failed.
    fn failed_first_start(
        responses: Vec<soyeht_ptyd::Result<Response>>,
    ) -> (
        Core,
        std::path::PathBuf,
        String,
        Arc<AtomicUsize>,
        Arc<Manual>,
    ) {
        let (mut core, directory, id, calls) = core_with_unstarted_pane(responses);
        let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
        core.clock = clock.clone();
        let spawned = core.spawn(&id);
        assert!(spawned.is_err());
        core.settle_spawn(&id, spawned);
        (core, directory, id, calls, clock)
    }

    #[test]
    fn a_transient_spawn_failure_is_retried_on_the_tick_and_the_pane_comes_up() {
        let [spawn, resize] = started();
        let (mut core, _directory, id, calls, clock) =
            failed_first_start(vec![failed_start(), spawn, resize]);
        assert_eq!(core.supervisor_retry.spawn_retries.attempts(&id), 1);
        assert!(core.panes[&id].stored.instance_id.is_none());

        // Inside the delay the tick does not touch the supervisor.
        assert_eq!(core.ensure_supervisor(), Link::Up);
        core.advance_spawn_retries().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        clock.0.fetch_add(10_000, Ordering::SeqCst);
        core.advance_spawn_retries().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            core.panes[&id].stored.instance_id.as_deref(),
            Some("instance")
        );
        assert_eq!(core.supervisor_retry.spawn_retries.attempts(&id), 0);
        assert!(core.ptyd.is_some());
        assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
        assert_eq!(events_named(&mut core, event::PTYD_RECONNECTED), 0);
    }

    #[test]
    fn a_spawn_that_keeps_failing_is_retried_a_bounded_number_of_times_and_ends_dead() {
        let (mut core, _directory, id, calls, clock) =
            failed_first_start((0..MAX_ATTEMPTS).map(|_| failed_start()).collect());
        for _ in 1..MAX_ATTEMPTS {
            assert_ne!(core.panes[&id].stored.pane.state, PaneState::Dead);
            clock.0.fetch_add(10_000, Ordering::SeqCst);
            core.advance_spawn_retries().unwrap();
        }
        let pane = &core.panes[&id].stored.pane;
        assert_eq!(pane.state, PaneState::Dead);
        assert_eq!(pane.phrase, NOT_STARTED);
        assert_eq!(core.supervisor_retry.spawn_retries.attempts(&id), 0);
        let asked = calls.load(Ordering::SeqCst);
        assert_eq!(asked, MAX_ATTEMPTS as usize);

        // It is not tried again, and the link was never dropped.
        clock.0.fetch_add(100_000, Ordering::SeqCst);
        core.advance_spawn_retries().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), asked);
        assert!(core.ptyd.is_some());
        assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
    }

    #[test]
    fn a_pane_that_closed_or_was_stopped_meanwhile_is_not_started_again() {
        for stop in [
            (|stored: &mut crate::model::StoredPane| stored.closed = true) as fn(&mut _),
            |stored| stored.pane.state = PaneState::Dead,
        ] {
            let (mut core, _directory, id, calls, clock) = failed_first_start(vec![failed_start()]);
            stop(&mut core.panes.get_mut(&id).unwrap().stored);
            clock.0.fetch_add(10_000, Ordering::SeqCst);
            core.advance_spawn_retries().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(core.supervisor_retry.spawn_retries.attempts(&id), 0);
        }
    }

    #[test]
    fn a_link_lost_while_a_start_is_pending_leaves_it_to_the_reconciliation() {
        let (mut core, _directory, id, _calls, _clock) = failed_first_start(vec![failed_start()]);
        // The next try meets a lost link: nothing is scheduled again.
        core.settle_spawn(&id, Err(remote(wire_kind::SPAWN_FAILED)));
        let attempts = core.supervisor_retry.spawn_retries.attempts(&id);
        let lost = core.spawn(&id);
        core.settle_spawn(&id, lost);
        assert!(core.ptyd.is_none());
        assert_eq!(core.supervisor_retry.spawn_retries.attempts(&id), attempts);
    }

    fn failed_close() -> soyeht_ptyd::Result<Response> {
        Err(PtydError::Remote {
            kind: wire_kind::OPERATION_FAILED.into(),
            message: "registry".into(),
        })
    }

    /// A core with a closed pane whose process the supervisor failed to close once; the
    /// supervisor then gives `then`.
    fn closed_pane_whose_close_failed(
        then: Vec<soyeht_ptyd::Result<Response>>,
    ) -> (Core, std::path::PathBuf, Arc<AtomicUsize>, Arc<Manual>) {
        let mut responses = vec![failed_close()];
        responses.extend(then);
        let (mut core, directory, id, calls) = core_with_unstarted_pane(responses);
        let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
        core.clock = clock.clone();
        let pane = core.panes.get_mut(&id).unwrap();
        pane.stored.instance_id = Some("process".into());
        pane.stored.closed = true;
        core.finish_close(&id).unwrap();
        (core, directory, calls, clock)
    }

    #[test]
    fn a_close_the_supervisor_failed_is_retried_on_the_tick_with_the_link_kept() {
        let (mut core, _directory, calls, clock) =
            closed_pane_whose_close_failed(vec![Ok(Response::Ok)]);
        assert!(core.ptyd.is_some());
        assert_eq!(core.supervisor_retry.close_retries.attempts("process"), 1);

        // Inside the delay the tick sends nothing.
        core.advance_close_retries().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        clock.0.fetch_add(10_000, Ordering::SeqCst);
        core.advance_close_retries().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(core.supervisor_retry.close_retries.attempts("process"), 0);
        assert!(core.ptyd.is_some());
        assert_eq!(events_named(&mut core, event::PTYD_LOST), 0);
    }

    #[test]
    fn a_close_that_keeps_failing_is_given_up_after_a_bounded_number_of_tries() {
        let (mut core, _directory, calls, clock) =
            closed_pane_whose_close_failed((1..MAX_ATTEMPTS).map(|_| failed_close()).collect());
        for _ in 1..MAX_ATTEMPTS {
            clock.0.fetch_add(10_000, Ordering::SeqCst);
            core.advance_close_retries().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS as usize);
        assert_eq!(core.supervisor_retry.close_retries.attempts("process"), 0);
        clock.0.fetch_add(100_000, Ordering::SeqCst);
        core.advance_close_retries().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS as usize);
        assert!(core.ptyd.is_some());
    }

    #[test]
    fn a_close_on_a_lost_link_is_left_to_the_reconciliation() {
        let (mut core, _directory, _id, _calls) = core_with_unstarted_pane(Vec::new());
        let closed = core.pty_request(soyeht_ptyd::wire::Request::Close {
            instance_id: "process".into(),
        });
        core.settle_close("process", closed.map(drop));
        assert!(core.ptyd.is_none());
        assert_eq!(core.supervisor_retry.close_retries.attempts("process"), 0);
    }
}
