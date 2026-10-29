//! Work that must not run while the Core lock is held (ADR 0079).
//!
//! Each slow operation (git, `wt`, `lsof`, usage sources, project walks) has
//! three steps: copy its inputs under the lock, do the work with no lock, and
//! apply the result under the lock. Two shapes carry those steps:
//!
//! - [`Offload`] answers one request. `dispatch` returns it in
//!   `Outcome::Offload`; the server runs the work on a blocking thread and then
//!   applies it under the lock to produce the reply.
//! - [`Background`] runs work the engine starts itself, such as polls, scans
//!   and dispatch steps, on a small worker pool. The tick applies the finished
//!   jobs under the lock.

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    streak::Streak,
};
use soyeht_platform::log::{self, Failure as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, PoisonError, mpsc},
    thread,
    time::{Duration, Instant},
};

/// Background jobs run concurrently on this many threads; more wait in line.
const BACKGROUND_WORKERS: usize = 4;
/// A job whose apply failed is not started again for this long.
const BACKGROUND_RETRY_AFTER: Duration = Duration::from_secs(1);

type ApplyOutcome = Box<dyn FnOnce(&mut Core) -> Result<Outcome> + Send>;
type ApplyJob = Box<dyn FnOnce(&mut Core) -> Result<()> + Send>;
type Task = Box<dyn FnOnce() + Send>;

/// A port that blocking work borrows off the Core lock. Its own mutex only
/// orders the users of this port; the Core lock is never held with it.
pub struct SharedPort<P: ?Sized>(Arc<Mutex<Box<P>>>);

impl<P: ?Sized> Clone for SharedPort<P> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<P: ?Sized> SharedPort<P> {
    pub fn new(port: Box<P>) -> Self {
        Self(Arc::new(Mutex::new(port)))
    }

    pub fn with<R>(&self, use_port: impl FnOnce(&mut P) -> Result<R>) -> Result<R> {
        let mut port = self
            .0
            .lock()
            .map_err(|_| errors::internal("Port lock poisoned"))?;
        use_port(&mut **port)
    }
}

/// The off-lock part of one request and the step that answers it.
pub struct Offload {
    /// The reply may come after the replies to the requests behind this one.
    concurrent: bool,
    timeout: Duration,
    work: Box<dyn FnOnce() -> ApplyOutcome + Send>,
    expire: ApplyOutcome,
}

impl Offload {
    /// `apply` always runs once under the lock: with the work's result, or
    /// with an internal error saying `timeout_message` when the work outlives
    /// `timeout` or fails. It can release what the request reserved.
    pub(crate) fn new<T, W, A>(
        timeout_message: &'static str,
        timeout: Duration,
        work: W,
        apply: A,
    ) -> Self
    where
        T: Send + 'static,
        W: FnOnce() -> Result<T> + Send + 'static,
        A: FnOnce(&mut Core, Result<T>) -> Result<Outcome> + Send + 'static,
    {
        let slot = Arc::new(Mutex::new(Some(apply)));
        let worker_slot = Arc::clone(&slot);
        Self {
            concurrent: false,
            timeout,
            work: Box::new(move || {
                let result = work();
                answer(&worker_slot, result)
            }),
            expire: answer(&slot, Err(errors::internal(timeout_message))),
        }
    }

    /// Lets the connection go on with the requests behind this one while the work runs, so
    /// a slow read does not hold back the ones that follow it (ADR 0095). Only for a request
    /// that no later request on the connection depends on: a read whose reply carries its id.
    pub(crate) fn concurrent(mut self) -> Self {
        self.concurrent = true;
        self
    }

    pub(crate) fn is_concurrent(&self) -> bool {
        self.concurrent
    }

    /// Runs both steps on this thread, for callers that hold no lock.
    #[cfg(test)]
    pub(crate) fn run_inline(self, core: &mut Core) -> Result<Outcome> {
        (self.work)()(core)
    }

    /// The deadline, the work, and the answer used when the work is late.
    pub(crate) fn into_parts(
        self,
    ) -> (
        Duration,
        Box<dyn FnOnce() -> ApplyOutcome + Send>,
        ApplyOutcome,
    ) {
        (self.timeout, self.work, self.expire)
    }
}

impl Outcome {
    /// Rewrites the reply this outcome gives, now or after its offloaded work.
    pub(crate) fn map_reply<M>(self, map: M) -> Result<Outcome>
    where
        M: FnOnce(serde_json::Value) -> Result<serde_json::Value> + Send + 'static,
    {
        match self {
            Outcome::Reply(value) => map(value).map(Outcome::Reply),
            Outcome::Offload(offload) => Ok(Outcome::Offload(offload.map_reply(map))),
            _ => Err(errors::internal("Outcome has no reply to rewrite")),
        }
    }
}

impl Offload {
    fn map_reply<M>(self, map: M) -> Self
    where
        M: FnOnce(serde_json::Value) -> Result<serde_json::Value> + Send + 'static,
    {
        let slot = Arc::new(Mutex::new(Some(map)));
        let worker_slot = Arc::clone(&slot);
        let work = self.work;
        Self {
            concurrent: self.concurrent,
            timeout: self.timeout,
            work: Box::new(move || mapped(work(), worker_slot)),
            expire: mapped(self.expire, slot),
        }
    }
}

/// Takes the value out of `slot`; only the first caller gets it. Nothing runs
/// while the slot is locked, so a poisoned slot still holds a valid value.
fn take_once<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// `apply`, with its reply rewritten by the map in `slot`.
fn mapped<M>(apply: ApplyOutcome, slot: Arc<Mutex<Option<M>>>) -> ApplyOutcome
where
    M: FnOnce(serde_json::Value) -> Result<serde_json::Value> + Send + 'static,
{
    Box::new(move |core: &mut Core| {
        let outcome = apply(core)?;
        match take_once(&slot) {
            Some(map) => outcome.map_reply(map),
            None => Err(errors::internal("Request was already answered")),
        }
    })
}

/// Binds `result` to the apply step if no other answer took it first.
fn answer<T, A>(slot: &Arc<Mutex<Option<A>>>, result: Result<T>) -> ApplyOutcome
where
    T: Send + 'static,
    A: FnOnce(&mut Core, Result<T>) -> Result<Outcome> + Send + 'static,
{
    let slot = Arc::clone(slot);
    Box::new(move |core: &mut Core| match take_once(&slot) {
        Some(apply) => apply(core, result),
        None => Err(errors::internal("Request was already answered")),
    })
}

/// Engine-initiated jobs, keyed so one key never runs twice at a time.
#[derive(Default)]
pub struct Background {
    /// Unit tests run work and apply in the caller, for determinism.
    #[cfg(test)]
    inline: bool,
    pool: Option<mpsc::Sender<Task>>,
    active: BTreeSet<String>,
    finished: Arc<Mutex<Vec<(String, ApplyJob)>>>,
    retry_after: BTreeMap<String, Instant>,
    failures: BTreeMap<String, Streak>,
    /// Keys whose next job runs on a thread of its own (see `run_alone`).
    alone: BTreeSet<String>,
    /// The signal of each job running alone, so a request can wait for it off the lock.
    done: BTreeMap<String, Arc<Done>>,
}

/// Set when a job on a thread of its own has ended and its result waits to be applied.
pub(crate) struct Done {
    ended: Mutex<bool>,
    changed: Condvar,
}

impl Done {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            ended: Mutex::new(false),
            changed: Condvar::new(),
        })
    }

    fn end(&self) {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    /// Waits up to `timeout` for the job to end; whether it did.
    pub(crate) fn wait(&self, timeout: Duration) -> bool {
        let ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
        let (ended, _) = self
            .changed
            .wait_timeout_while(ended, timeout, |ended| !*ended)
            .unwrap_or_else(PoisonError::into_inner);
        *ended
    }
}

impl Background {
    #[cfg(test)]
    pub(crate) fn inline() -> Self {
        Self {
            inline: true,
            ..Self::default()
        }
    }

    pub(crate) fn is_active(&self, key: &str) -> bool {
        self.active.contains(key)
    }

    /// Whether `spawn_background` would start `key` now: it is neither running nor cooling
    /// down after a failure.
    pub(crate) fn accepts(&self, key: &str) -> bool {
        !self.active.contains(key)
            && self
                .retry_after
                .get(key)
                .is_none_or(|after| *after <= Instant::now())
    }

    /// Whether any job is running or waiting to be applied.
    #[cfg(test)]
    pub(crate) fn has_work(&self) -> bool {
        !self.active.is_empty() || self.finished.lock().is_ok_and(|done| !done.is_empty())
    }

    /// Makes the next job for `key` run on a thread of its own, for work that waits on a disk
    /// that may hang: it never holds one of the shared workers, so a hung disk cannot starve the
    /// terminal's flushes. Its key stays running until the work ends.
    pub(crate) fn run_alone(&mut self, key: &str) {
        self.alone.insert(key.to_owned());
    }

    fn submit(&mut self, task: Task, own_thread: bool) -> Result<()> {
        if own_thread {
            // A job that may block for as long as a disk does must not take one of the shared
            // workers, which the terminal path also needs.
            return thread::Builder::new()
                .name("soyeht-background-own".into())
                .spawn(task)
                .map(drop)
                .map_err(errors::io);
        }
        let sender = match &mut self.pool {
            Some(sender) => sender,
            None => self.pool.insert(start_pool()?),
        };
        sender
            .send(task)
            .map_err(|_| errors::internal("Background workers stopped"))
    }

    /// The jobs running alone, by key, with their signals.
    pub(crate) fn alone_jobs(&self) -> impl Iterator<Item = (&String, &Arc<Done>)> {
        self.done.iter()
    }

    /// The signal of the job running alone for `key`, if one is.
    #[cfg(test)]
    pub(crate) fn done_of(&self, key: &str) -> Option<Arc<Done>> {
        self.done.get(key).cloned()
    }

    fn settle(&mut self, key: &str, result: &Result<()>, now: Instant) {
        self.active.remove(key);
        self.done.remove(key);
        if result.is_ok() {
            self.retry_after.remove(key);
        } else {
            self.retry_after
                .insert(key.to_owned(), now + BACKGROUND_RETRY_AFTER);
        }
        self.report(key, result);
    }

    /// Logs a job's or a tick subsystem's failure once per change of kind,
    /// and its recovery.
    pub(crate) fn report(&mut self, key: &str, result: &Result<()>) {
        match result {
            Ok(()) => {
                if self.failures.remove(key).is_some() {
                    log::event(
                        "engine.subsystem_recovered",
                        &[("subsystem", subsystem(key))],
                    );
                }
            }
            Err(error) => {
                let kind = error.failure_kind();
                let streak = self
                    .failures
                    .entry(key.to_owned())
                    .or_insert_with(Streak::once);
                if streak.fail(&kind) {
                    log::event(
                        "engine.subsystem_failed",
                        &[("subsystem", subsystem(key)), ("kind", &kind)],
                    );
                }
            }
        }
    }
}

fn start_pool() -> Result<mpsc::Sender<Task>> {
    let (sender, receiver) = mpsc::channel::<Task>();
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..BACKGROUND_WORKERS {
        let receiver = Arc::clone(&receiver);
        thread::Builder::new()
            .name(format!("soyeht-background-{index}"))
            .spawn(move || {
                loop {
                    let task = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        // ignored: A worker that panicked while waiting for a task poisoned the queue.
                        Err(_) => return,
                    };
                    match task {
                        Ok(task) => task(),
                        // ignored: every sender is gone: the engine is shutting down.
                        Err(_) => return,
                    }
                }
            })
            .map_err(errors::io)?;
    }
    Ok(sender)
}

/// The subsystem of a key, without task or workspace identifiers.
fn subsystem(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

fn run_caught<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(errors::internal("Background work panicked")))
}

impl Core {
    /// Starts `work` with no lock unless `key` is running or cooling down
    /// after a failure. `apply` gets its result under the lock on a later
    /// tick. Inline (test) mode returns the apply result instead.
    pub(crate) fn spawn_background<T, W, A>(&mut self, key: String, work: W, apply: A) -> Result<()>
    where
        T: Send + 'static,
        W: FnOnce() -> Result<T> + Send + 'static,
        A: FnOnce(&mut Core, Result<T>) -> Result<()> + Send + 'static,
    {
        let own_thread = self.background.alone.remove(&key);
        let now = Instant::now();
        if !self.background.accepts(&key) {
            return Ok(());
        }
        self.background.active.insert(key.clone());
        #[cfg(test)]
        if self.background.inline {
            let result = run_caught(work);
            let applied = apply(self, result);
            self.background.active.remove(&key);
            return applied;
        }
        let finished = Arc::clone(&self.background.finished);
        let job_key = key.clone();
        let done = own_thread.then(Done::new);
        if let Some(done) = &done {
            self.background.done.insert(key.clone(), Arc::clone(done));
        }
        let submitted = self.background.submit(
            Box::new(move || {
                let result = run_caught(work);
                let job: ApplyJob = Box::new(move |core| apply(core, result));
                // The list only gains and loses whole jobs, so a poisoned lock still holds a
                // valid one; dropping the job here would leave its key active for good.
                finished
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((job_key, job));
                if let Some(done) = done {
                    done.end();
                }
            }),
            own_thread,
        );
        if let Err(error) = &submitted {
            self.background
                .settle(&key, &Err(errors::internal(error.to_string())), now);
        }
        submitted
    }

    /// Applies every finished background job. A failure is logged for its
    /// job alone and never stops the others or the tick.
    pub(crate) fn apply_background(&mut self) {
        // The list only gains and loses whole jobs, so a poisoned lock still holds a valid one.
        let finished = std::mem::take(
            &mut *self
                .background
                .finished
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for (key, apply) in finished {
            let result = apply(self);
            self.background.settle(&key, &result, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn core() -> (Core, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("soyeht-offload-{}", uuid::Uuid::new_v4()));
        (Core::open(&directory).unwrap(), directory)
    }

    #[test]
    fn a_pooled_job_runs_without_the_lock_and_applies_on_a_later_tick() {
        let (core, directory) = core();
        let shared = Arc::new(Mutex::new(core));
        let (release, gate) = mpsc::channel::<()>();
        let applied = Arc::new(AtomicUsize::new(0));
        {
            let counter = Arc::clone(&applied);
            let mut core = shared.lock().unwrap();
            core.spawn_background(
                "test.slow:1".into(),
                move || {
                    gate.recv_timeout(Duration::from_secs(30)).ok();
                    Ok(7)
                },
                move |_, value| {
                    assert_eq!(value.unwrap(), 7);
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .unwrap();
            // The same key is not started twice while it runs.
            core.spawn_background("test.slow:1".into(), || Ok(0), |_, _| panic!("duplicate"))
                .unwrap();
        }
        // The worker is blocked, yet the lock is free and ticks proceed.
        for _ in 0..3 {
            let mut core = shared.try_lock().unwrap();
            core.apply_background();
            assert!(core.background.is_active("test.slow:1"));
        }
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while applied.load(Ordering::SeqCst) == 0 {
            assert!(
                Instant::now() < deadline,
                "the finished job was never applied"
            );
            shared.lock().unwrap().apply_background();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!shared.lock().unwrap().background.is_active("test.slow:1"));
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_failed_job_waits_before_it_runs_again() {
        let (mut core, directory) = core();
        core.background = Background::inline();
        let runs = Arc::new(AtomicUsize::new(0));
        let failing = |core: &mut Core, runs: &Arc<AtomicUsize>| {
            let counter = Arc::clone(runs);
            core.spawn_background(
                "test.failing".into(),
                move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(errors::internal("failed"))
                },
                |_, result| result,
            )
        };
        let failed = failing(&mut core, &runs);
        assert!(failed.is_err());
        core.background
            .settle("test.failing", &failed, Instant::now());
        assert!(failing(&mut core, &runs).is_ok());
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_offload_answers_once_even_when_its_work_finishes_after_the_deadline() {
        let (mut core, directory) = core();
        let offload = Offload::new(
            "test work timed out",
            Duration::from_millis(1),
            || Ok(1),
            |_, result: Result<i32>| result.map(|value| Outcome::Reply(value.into())),
        );
        let (_, work, expire) = offload.into_parts();
        let late = work();
        let Err(errors::Error::Internal(message)) = expire(&mut core) else {
            panic!("the expired request did not report its timeout")
        };
        assert_eq!(message, "test work timed out");
        assert!(late(&mut core).is_err());
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
