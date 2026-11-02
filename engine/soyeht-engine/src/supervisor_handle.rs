//! The engine's link to the supervisor, as a handle that waiting borrows off the Core lock
//! (ADR 0117).
//!
//! A request to the supervisor waits for its answer for up to [`IO_TIMEOUT`], and the terminal
//! output path shares the Core lock with every request, so a request that waits under the lock
//! stalls every terminal while the supervisor is slow. The link is therefore a [`Supervisor`]:
//! a cheap clone that owns the connection behind a mutex of its own. Work copies its inputs
//! under the lock, calls through its clone off the lock (`Outcome::Offload` for a request,
//! `Core::spawn_background` for the engine's own work), and settles the answer under the lock
//! with [`Core::settle_supervisor`].
//!
//! The connection is a single stream of requests and answers, so it must never be used again
//! after a call that did not finish cleanly: a late answer would be read as the answer of the
//! next request. A call that fails with anything but the supervisor's own refusal (a timeout,
//! a transport or framing failure, a panic) marks the handle broken, inside the mutex, and
//! every later call on it fails at once with the link-lost error, without touching the stream.
//! The tick drops a broken link at once; a call that outlived the link says nothing about the
//! one that replaced it ([`Core::settle_supervisor`]).
//!
//! What still waits under the Core lock is counted by `tests/blocking_io_rule.rs`.

use crate::{
    core::Core,
    errors::{self, Error, Result},
    offload::Done,
    ports::{SupervisorPort, call_supervisor},
};
use soyeht_ptyd::{
    client::IO_TIMEOUT,
    wire::{Request, Response},
};
use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// How long a request answered off the lock may take, from the start of its work: the wait
/// for the sends ahead of it, then its own write and read.
pub(crate) const DEADLINE: Duration = IO_TIMEOUT.saturating_mul(6);

/// The background job that sends the follow-ups nobody waits for.
const LATER_JOB: &str = "supervisor.later";
/// The log event of a follow-up that failed.
const LATER_FAILED: &str = "engine.supervisor_follow_up_failed";
/// The log event of a follow-up job that could not be started.
const LATER_NOT_STARTED: &str = "engine.supervisor_follow_up_not_started";

struct Connection {
    port: Mutex<Box<dyn SupervisorPort>>,
    /// Set, with the mutex held, by a call that left the stream in an unknown state. Read
    /// without the mutex, so the tick can drop the link while a call is still waiting on it.
    broken: AtomicBool,
}

/// The connection to the supervisor. Clones share it.
#[derive(Clone)]
pub struct Supervisor {
    connection: Arc<Connection>,
    id: Arc<str>,
    generation: u32,
}

impl Supervisor {
    pub fn new(port: Box<dyn SupervisorPort>) -> Self {
        Self {
            id: Arc::from(port.supervisor_id()),
            generation: port.supervisor_generation(),
            connection: Arc::new(Connection {
                port: Mutex::new(port),
                broken: AtomicBool::new(false),
            }),
        }
    }

    pub fn supervisor_id(&self) -> &str {
        &self.id
    }

    pub fn supervisor_generation(&self) -> u32 {
        self.generation
    }

    /// Whether a call left the stream in an unknown state. Such a handle only fails.
    pub(crate) fn is_broken(&self) -> bool {
        self.connection.broken.load(Ordering::SeqCst)
    }

    /// The error of a call on a broken handle: the one a lost link gives.
    fn gone() -> Error {
        Error::Supervisor(soyeht_ptyd::Error::Io(
            std::io::ErrorKind::BrokenPipe.into(),
        ))
    }

    /// Sends one request and waits for the answer: up to [`IO_TIMEOUT`] for the request itself,
    /// plus the wait for the callers ahead of it, which end at the first call that breaks the
    /// handle. Never call it under the Core lock from a path that was moved off it.
    pub fn call(&self, request: Request) -> Result<Response> {
        if self.is_broken() {
            return Err(Self::gone());
        }
        // A panic inside a call leaves the stream in an unknown state too.
        let mut port = self.connection.port.lock().unwrap_or_else(|poisoned| {
            self.connection.broken.store(true, Ordering::SeqCst);
            PoisonError::into_inner(poisoned)
        });
        // The caller ahead may have broken it while this one waited.
        if self.is_broken() {
            return Err(Self::gone());
        }
        let result = call_supervisor(port.as_mut(), request);
        if result
            .as_ref()
            .err()
            .is_some_and(errors::supervisor_link_lost)
        {
            self.connection.broken.store(true, Ordering::SeqCst);
        }
        result
    }

    /// Sends `batch` in order. A request the supervisor refuses does not stop the others; the
    /// first refusal is returned after them. A lost link stops the batch at once, since the
    /// rest cannot be sent.
    pub(crate) fn call_each(&self, batch: Vec<Request>) -> Result<()> {
        let mut first = Ok(());
        for request in batch {
            if let Err(error) = self.call(request) {
                if errors::supervisor_link_lost(&error) {
                    return Err(error);
                }
                if first.is_ok() {
                    first = Err(error);
                }
            }
        }
        first
    }

    /// The work of an input: waits for its turn, sends the follow-ups queued for the process
    /// that it carried (in front of its own bytes), then the input. `None` when the request ran
    /// out of time before its turn came: nothing was sent.
    pub(crate) fn send_input(
        &self,
        turn: &Turn,
        carried: &Carried,
        request: Request,
    ) -> Option<(Result<()>, Result<Response>)> {
        turn.wait();
        let earlier = carried.take()?;
        let earlier = self.call_each(earlier);
        Some((earlier, self.call(request)))
    }

    /// Whether `other` is the same connection, not a later one to the same supervisor.
    pub(crate) fn same_link(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.connection, &other.connection)
    }
}

/// The process a request is about, for the requests whose order against each other matters.
fn instance_of(request: &Request) -> Option<&str> {
    match request {
        Request::Resize { instance_id, .. }
        | Request::Close { instance_id }
        | Request::Ack { instance_id, .. } => Some(instance_id),
        _ => None,
    }
}

/// A place in the order of the sends to some processes. Sends reach the supervisor in the order
/// their turns were taken under the Core lock, whichever thread makes them: a turn waits for
/// the turns taken before it for the same processes, and ends when it is dropped, whether its
/// work ran, failed, panicked or never started.
pub(crate) struct Turn {
    before: Vec<Arc<Done>>,
    mine: Arc<Done>,
}

impl Turn {
    /// Waits until the sends that were ahead of this one are over, or [`DEADLINE`] has passed.
    pub(crate) fn wait(&self) {
        for earlier in &self.before {
            earlier.wait(DEADLINE);
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.mine.end();
    }
}

/// Sends taken out of the queue by an input, until the input's work takes them to send them in
/// front of its own bytes. If the work never starts (the request ran out of time first), the
/// answer to the request puts them back.
#[derive(Clone)]
pub(crate) struct Carried(Arc<Mutex<Option<Vec<Request>>>>);

impl Carried {
    /// The sends, if nobody took them yet; only the first caller gets them.
    pub(crate) fn take(&self) -> Option<Vec<Request>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl Core {
    /// Makes `port` the link to the supervisor, for a test that scripts the other end.
    #[cfg(test)]
    pub(crate) fn link_supervisor(&mut self, port: Box<dyn SupervisorPort>) {
        self.ptyd = Some(Supervisor::new(port));
    }

    /// A handle to the current link, for work that waits off the Core lock.
    pub(crate) fn supervisor(&self) -> Result<Supervisor> {
        self.ptyd
            .clone()
            .ok_or_else(|| errors::ptyd_unavailable(soyeht_protocol::SUPERVISOR_DISCONNECTED))
    }

    /// Whether `link` is the engine's link now, not one that was replaced.
    pub(crate) fn is_current_link(&self, link: &Supervisor) -> bool {
        self.ptyd
            .as_ref()
            .is_some_and(|current| current.same_link(link))
    }

    /// Takes the answer of a call made through `used` back under the lock. An error that says
    /// the link is gone drops it, unless it is already another link than the one `used` called
    /// (a call that outlived the link says nothing about the new one). The caller still gets
    /// `ptyd_unavailable`.
    pub(crate) fn settle_supervisor<T>(
        &mut self,
        used: &Supervisor,
        result: Result<T>,
    ) -> Result<T> {
        match result {
            Err(error) if errors::supervisor_link_lost(&error) => {
                if self.is_current_link(used) {
                    self.lose_supervisor_link();
                }
                Err(errors::ptyd_unavailable(
                    soyeht_protocol::SUPERVISOR_DISCONNECTED,
                ))
            }
            result => result,
        }
    }

    /// [`Core::settle_supervisor`] for an answer that means nothing once its link is gone: a
    /// failure of a call made on a link that is no longer the current one is dropped (`None`),
    /// so it neither fails the job that carries it nor cools it down after a reconnect.
    pub(crate) fn settle_if_current<T>(
        &mut self,
        used: &Supervisor,
        result: Result<T>,
    ) -> Result<Option<T>> {
        let current = self.is_current_link(used);
        let settled = self.settle_supervisor(used, result);
        if !current && settled.is_err() {
            return Ok(None);
        }
        settled.map(Some)
    }

    /// Settles the answer of sends nobody waits for: a failure is logged under
    /// `engine.supervisor_follow_up_failed`, once per change of kind.
    pub(crate) fn settle_follow_ups(&mut self, used: &Supervisor, result: Result<()>) {
        let settled = self.settle_supervisor(used, result);
        self.settle_supervisor_call(LATER_FAILED, settled);
    }

    /// Takes the receipt of an input back under the lock: puts back what the work did not send,
    /// settles the follow-ups it carried and the receipt, and says how many bytes the supervisor
    /// accepted. `None` when the receipt is a failure of a link that was replaced meanwhile.
    pub(crate) fn settle_input(
        &mut self,
        used: &Supervisor,
        carried: &Carried,
        sent: Option<(Result<()>, Result<Response>)>,
    ) -> Result<Option<usize>> {
        self.requeue_later(carried);
        let (earlier, receipt) =
            sent.ok_or_else(|| errors::internal("Input timed out before its turn"))?;
        self.settle_follow_ups(used, earlier);
        match self.settle_if_current(used, receipt)? {
            Some(Response::Accepted { bytes }) => Ok(Some(bytes)),
            Some(_) => Err(errors::ptyd_unavailable("Invalid input receipt")),
            None => Ok(None),
        }
    }

    /// [`Core::settle_input`] for the end of an input job, whether or not its work gave an answer:
    /// the sends the work did not take go back first, so a job that panicked before it took them
    /// does not lose them.
    fn settle_started_input(
        &mut self,
        used: &Supervisor,
        carried: &Carried,
        sent: Result<Option<(Result<()>, Result<Response>)>>,
    ) -> Result<Option<usize>> {
        self.requeue_later(carried);
        self.settle_input(used, carried, sent?)
    }

    /// Sends the input `request` to the process `instance_id` off the Core lock, as the job
    /// `job`, in the order of the sends to that process (the queued ones go in front of it), and
    /// gives `done` the bytes the supervisor accepted under the lock; `None` for a failure of a
    /// link that was replaced meanwhile. Nothing is sent when `job` is running.
    pub(crate) fn start_input(
        &mut self,
        job: String,
        instance_id: &str,
        request: Request,
        done: impl FnOnce(&mut Core, Result<Option<usize>>) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let link = self.supervisor()?;
        let used = link.clone();
        let carried = self.take_later_for(instance_id);
        let turn = self.turn(&[instance_id]);
        let taken = carried.clone();
        let requeue = carried.clone();
        let started = self.spawn_prompt_background(
            job,
            move || Ok(link.send_input(&turn, &taken, request)),
            move |core, sent| {
                let accepted = core.settle_started_input(&used, &carried, sent);
                done(core, accepted)
            },
            |_| true,
        );
        if started.is_err() {
            // The job never ran: the sends it took go back.
            self.requeue_later(&requeue);
        }
        started
    }

    /// Takes a place in the order of the sends to `instances`.
    pub(crate) fn turn(&mut self, instances: &[&str]) -> Turn {
        self.send_turns.retain(|_, done| !done.is_ended());
        let mine = Done::new();
        let mut before: Vec<Arc<Done>> = Vec::new();
        for instance in instances {
            let earlier = self
                .send_turns
                .insert((*instance).to_owned(), Arc::clone(&mine));
            if let Some(earlier) = earlier
                && !before.iter().any(|seen| Arc::ptr_eq(seen, &earlier))
            {
                before.push(earlier);
            }
        }
        Turn { before, mine }
    }

    /// Sends `request` to the supervisor without waiting for it or its answer, for what the
    /// caller does not depend on (a confirmation, a size it already made durable). Requests go
    /// in the order given, and a size not yet sent is replaced by the newer one for the same
    /// process. Sends to a process stay in order with an input to it (see
    /// [`Core::take_later_for`]).
    pub(crate) fn send_later(&mut self, request: Request) {
        if self.ptyd.is_none() {
            return;
        }
        // A newer size or acknowledgement of a process replaces the one not yet sent.
        match &request {
            Request::Resize { instance_id, .. } => self.supervisor_later.retain(|queued| {
                !matches!(queued, Request::Resize { instance_id: other, .. } if other == instance_id)
            }),
            Request::Ack { instance_id, .. } => self.supervisor_later.retain(|queued| {
                !matches!(queued, Request::Ack { instance_id: other, .. } if other == instance_id)
            }),
            _ => {}
        }
        self.supervisor_later.push(request);
        self.flush_later();
    }

    /// Drops the queued sends to `instance`, which is being closed: they would find it gone.
    pub(crate) fn forget_later_for(&mut self, instance: &str) {
        self.supervisor_later
            .retain(|queued| instance_of(queued) != Some(instance));
    }

    /// Takes the queued sends to `instance` out of the queue, for the input that is about to
    /// follow them: the input sends them itself, in front of its own bytes, after the sends
    /// already on their way.
    pub(crate) fn take_later_for(&mut self, instance: &str) -> Carried {
        let (mine, rest) = std::mem::take(&mut self.supervisor_later)
            .into_iter()
            .partition(|request| instance_of(request) == Some(instance));
        self.supervisor_later = rest;
        Carried(Arc::new(Mutex::new(Some(mine))))
    }

    /// Puts back the sends an input took and its work never sent, in front of the queue. A size
    /// that a newer one has replaced meanwhile is dropped.
    pub(crate) fn requeue_later(&mut self, carried: &Carried) {
        let Some(unsent) = carried.take() else {
            return;
        };
        let mut queue: Vec<Request> = unsent
            .into_iter()
            .filter(|request| {
                !matches!(request, Request::Resize { instance_id, .. }
                    if self.supervisor_later.iter().any(|queued|
                        matches!(queued, Request::Resize { instance_id: other, .. } if other == instance_id)))
            })
            .collect();
        queue.append(&mut self.supervisor_later);
        self.supervisor_later = queue;
        self.flush_later();
    }

    /// Starts the job that sends the queued follow-ups, if none is running. The tick calls it
    /// too, for what arrived while one ran.
    pub(crate) fn flush_later(&mut self) {
        if self.supervisor_later.is_empty() || !self.background.accepts(LATER_JOB) {
            return;
        }
        let Ok(supervisor) = self.supervisor() else {
            self.supervisor_later.clear();
            return;
        };
        let batch = std::mem::take(&mut self.supervisor_later);
        let instances: Vec<&str> = batch.iter().filter_map(instance_of).collect();
        let turn = self.turn(&instances);
        let used = supervisor.clone();
        // On a thread of its own, not a shared worker: this job holds a turn that a typed input
        // waits for, so it must never wait for a worker itself.
        self.background.run_alone(LATER_JOB);
        let started = self.spawn_background(
            LATER_JOB.into(),
            move || {
                turn.wait();
                supervisor.call_each(batch)
            },
            move |core, sent| {
                core.settle_follow_ups(&used, sent);
                Ok(())
            },
        );
        self.settle_supervisor_call(LATER_NOT_STARTED, started);
    }
}

#[cfg(test)]
mod tests;
