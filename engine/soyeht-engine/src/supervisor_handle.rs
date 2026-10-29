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

    /// Whether `other` is the same connection, not a later one to the same supervisor.
    fn same_link(&self, other: &Self) -> bool {
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

impl Core {
    /// Makes `port` the link to the supervisor.
    pub fn link_supervisor(&mut self, port: Box<dyn SupervisorPort>) {
        self.ptyd = Some(Supervisor::new(port));
    }

    /// A handle to the current link, for work that waits off the Core lock.
    pub(crate) fn supervisor(&self) -> Result<Supervisor> {
        self.ptyd
            .clone()
            .ok_or_else(|| errors::ptyd_unavailable(soyeht_protocol::SUPERVISOR_DISCONNECTED))
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
                if self
                    .ptyd
                    .as_ref()
                    .is_some_and(|current| current.same_link(used))
                {
                    self.lose_supervisor_link();
                }
                Err(errors::ptyd_unavailable(
                    soyeht_protocol::SUPERVISOR_DISCONNECTED,
                ))
            }
            result => result,
        }
    }

    /// Settles the answer of sends nobody waits for: a failure is logged under
    /// `engine.supervisor_follow_up_failed`, once per change of kind.
    pub(crate) fn settle_follow_ups(&mut self, used: &Supervisor, result: Result<()>) {
        let settled = self.settle_supervisor(used, result);
        self.settle_supervisor_call(LATER_FAILED, settled);
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
        if let Request::Resize { instance_id, .. } = &request {
            self.supervisor_later.retain(|queued| {
                !matches!(queued, Request::Resize { instance_id: other, .. } if other == instance_id)
            });
        }
        self.supervisor_later.push(request);
        self.flush_later();
    }

    /// Takes the queued sends to `instance` out of the queue, for the input that is about to
    /// follow them: the input sends them itself, in front of its own bytes, after the sends
    /// already on their way.
    pub(crate) fn take_later_for(&mut self, instance: &str) -> Vec<Request> {
        let (mine, rest) = std::mem::take(&mut self.supervisor_later)
            .into_iter()
            .partition(|request| instance_of(request) == Some(instance));
        self.supervisor_later = rest;
        mine
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
mod tests {
    use super::*;
    use crate::{
        core_lock, dispatch::Outcome, errors::Error, ports::testing::FakeSupervisor,
        test_panes::insert_terminal_pane,
    };
    use serde_json::json;
    use soyeht_ptyd::wire::Output;
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Instant,
    };

    /// How long the slow supervisor takes to answer a drain.
    const SLOW: Duration = Duration::from_millis(1_500);

    /// A supervisor that says what it was asked. A drain takes `delay` to answer; a resize
    /// waits for `gate` when there is one.
    struct Stub {
        delay: Duration,
        draining: Arc<AtomicBool>,
        asked: Arc<Mutex<Vec<String>>>,
        gate: Option<Mutex<mpsc::Receiver<()>>>,
    }

    impl Stub {
        fn new(delay: Duration) -> (Self, Arc<AtomicBool>, Arc<Mutex<Vec<String>>>) {
            let draining = Arc::new(AtomicBool::new(false));
            let asked = Arc::new(Mutex::new(Vec::new()));
            let stub = Self {
                delay,
                draining: Arc::clone(&draining),
                asked: Arc::clone(&asked),
                gate: None,
            };
            (stub, draining, asked)
        }
    }

    impl SupervisorPort for Stub {
        fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
            let mut asked = self.asked.lock().unwrap();
            Ok(match request {
                Request::Drain { from_seq, .. } => {
                    asked.push("drain".into());
                    drop(asked);
                    self.draining.store(true, Ordering::SeqCst);
                    thread::sleep(self.delay);
                    Response::Output(Output {
                        from_seq,
                        next_seq: from_seq,
                        end_seq: from_seq,
                        bytes: String::new(),
                        gap: None,
                        exited: false,
                    })
                }
                Request::Input { bytes, .. } => {
                    asked.push(format!("input:{bytes}"));
                    Response::Accepted { bytes: bytes.len() }
                }
                Request::ConfirmInput { .. } => {
                    asked.push("confirm".into());
                    Response::Ok
                }
                Request::Resize {
                    instance_id, cols, ..
                } => {
                    asked.push(format!("{instance_id}:{cols}"));
                    drop(asked);
                    if let Some(gate) = &self.gate {
                        // A timeout only ends the wait; the test asserts what was sent.
                        gate.lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(30))
                            .ok();
                    }
                    Response::Ok
                }
                _ => Response::Ok,
            })
        }

        fn supervisor_id(&self) -> &str {
            "stub"
        }

        fn supervisor_generation(&self) -> u32 {
            1
        }
    }

    fn open_core() -> (Core, std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!("soyeht-handle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut core = Core::open(&root).unwrap();
        let pane = insert_terminal_pane(&mut core, &root, "shell", false);
        let (outbox, _receiver) = crate::outbox::channel();
        let connection = core.connect(true, false, outbox).unwrap();
        core.dispatch(
            &connection,
            json!({"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"handle-test","clientVersion":"1","protocolVersion":1}}),
        )
        .unwrap();
        (core, root, format!("{connection}|{pane}"))
    }

    fn request(id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
    }

    #[test]
    fn a_slow_supervisor_holds_up_neither_the_tick_nor_a_request_that_does_not_need_it() {
        let (mut core, root, ids) = open_core();
        let (connection, pane) = ids.split_once('|').unwrap();
        let (connection, pane) = (connection.to_owned(), pane.to_owned());
        let (stub, draining, asked) = Stub::new(SLOW);
        core.link_supervisor(Box::new(stub));
        let shared = Arc::new(Mutex::new(core));

        // The tick starts the read and goes on.
        let started = Instant::now();
        core_lock::lock(&shared, "test")
            .unwrap()
            .poll_output()
            .unwrap();
        let tick = started.elapsed();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !draining.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "the supervisor was never asked");
            thread::sleep(Duration::from_millis(1));
        }

        // While the supervisor is slow, the lock is free: a request that does not need the
        // supervisor is answered, and so is one that does, which waits off the lock.
        let started = Instant::now();
        let status = core_lock::lock(&shared, "test")
            .unwrap()
            .dispatch(&connection, request(2, "engine.status", json!({})));
        let status_latency = started.elapsed();
        assert!(matches!(status, Ok(Outcome::Reply(_))));
        let started = Instant::now();
        let input = core_lock::lock(&shared, "test").unwrap().dispatch(
            &connection,
            request(
                3,
                "pane.input",
                json!({"paneId":pane,"bytes":"bHMK","idempotencyKey":"typed"}),
            ),
        );
        let input_latency = started.elapsed();
        let Ok(Outcome::Offload(offload)) = input else {
            panic!("pane.input did not offload its wait for the supervisor")
        };

        assert!(
            tick < SLOW / 3,
            "the tick waited for the supervisor: {tick:?}"
        );
        assert!(
            status_latency < SLOW / 3,
            "a request waited behind the supervisor: {status_latency:?}"
        );
        assert!(
            input_latency < SLOW / 3,
            "pane.input waited for the supervisor under the lock: {input_latency:?}"
        );
        // The input is the next to use the connection, after the read it queued behind.
        let (_, work, _) = offload.into_parts();
        let applied = work();
        drop(applied);
        assert_eq!(*asked.lock().unwrap(), ["drain", "input:bHMK"]);
        drop(shared);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_input_is_confirmed_once_its_receipt_is_durable() {
        let (mut core, root, ids) = open_core();
        let (connection, pane) = ids.split_once('|').unwrap();
        core.background = crate::offload::Background::inline();
        let (stub, _, asked) = Stub::new(Duration::ZERO);
        core.link_supervisor(Box::new(stub));

        let reply = core.dispatch_inline(
            connection,
            request(
                2,
                "pane.input",
                json!({"paneId":pane,"bytes":"bHMK","idempotencyKey":"typed"}),
            ),
        );
        let Ok(Outcome::Reply(reply)) = reply else {
            panic!("pane.input did not reply: {:?}", reply.err())
        };

        assert_eq!(reply["acceptedBytes"], 4);
        assert_eq!(*asked.lock().unwrap(), ["input:bHMK", "confirm"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn follow_ups_go_in_order_and_a_size_not_yet_sent_is_replaced() {
        let (mut core, root, _) = open_core();
        let (release, gate) = mpsc::channel();
        let (mut stub, _, sizes) = Stub::new(Duration::ZERO);
        stub.gate = Some(Mutex::new(gate));
        core.link_supervisor(Box::new(stub));
        let resize = |instance: &str, cols| Request::Resize {
            instance_id: instance.into(),
            cols,
            rows: 24,
        };

        core.send_later(resize("a", 80));
        let deadline = Instant::now() + Duration::from_secs(30);
        while sizes.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "the first size was never sent");
            thread::sleep(Duration::from_millis(1));
        }
        // The first is in flight; these wait for it. The second size of "b" replaces the first.
        core.send_later(resize("b", 90));
        core.send_later(resize("b", 100));
        core.send_later(resize("a", 110));
        for _ in 0..3 {
            release.send(()).unwrap();
        }
        // What waited is sent by the job the tick starts once the first has ended.
        for _ in 0..2 {
            while core.background.is_active(LATER_JOB) {
                assert!(Instant::now() < deadline, "the follow-ups were never sent");
                core.apply_background();
                thread::sleep(Duration::from_millis(1));
            }
            core.flush_later();
        }
        while core.background.is_active(LATER_JOB) {
            core.apply_background();
            thread::sleep(Duration::from_millis(1));
        }

        assert_eq!(*sizes.lock().unwrap(), ["a:80", "b:100", "a:110"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_lost_link_is_dropped_only_by_the_call_that_used_it() {
        let (mut core, root, _) = open_core();
        core.link_supervisor(Box::new(FakeSupervisor::default()));
        let old = core.supervisor().unwrap();
        core.link_supervisor(Box::new(FakeSupervisor::default()));
        let lost = || Err::<(), _>(Error::Supervisor(soyeht_ptyd::Error::InvalidResponse));

        // A call that outlived its link says nothing about the one that replaced it.
        let outcome = core.settle_supervisor(&old, lost());
        assert!(
            matches!(outcome, Err(error) if errors::kind(&error) == soyeht_protocol::ErrorKind::PtydUnavailable)
        );
        assert!(core.ptyd.is_some());

        let current = core.supervisor().unwrap();
        assert!(core.settle_supervisor(&current, lost()).is_err());
        assert!(core.ptyd.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_handle_reports_the_identity_it_was_made_with() {
        let supervisor = Supervisor::new(Box::new(FakeSupervisor::default()));

        assert_eq!(
            supervisor.supervisor_id(),
            FakeSupervisor::default().supervisor_id()
        );
        assert!(supervisor.same_link(&supervisor.clone()));
        assert!(!supervisor.same_link(&Supervisor::new(Box::new(FakeSupervisor::default()))));
    }

    /// A supervisor whose first answer never comes in time: it fails with the timeout of the
    /// client after `delay`. A later request it is handed gets an answer of the wrong kind,
    /// as the late answer of an earlier one would be.
    struct TimesOut {
        delay: Duration,
        calls: Arc<AtomicUsize>,
    }

    impl SupervisorPort for TimesOut {
        fn request(&mut self, _: Request) -> soyeht_ptyd::Result<Response> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                thread::sleep(self.delay);
                return Err(soyeht_ptyd::Error::Io(std::io::ErrorKind::TimedOut.into()));
            }
            Ok(Response::Accepted { bytes: 999 })
        }

        fn supervisor_id(&self) -> &str {
            "times-out"
        }

        fn supervisor_generation(&self) -> u32 {
            1
        }
    }

    #[test]
    fn a_call_that_times_out_breaks_the_handle_and_no_late_answer_is_read_as_another() {
        let calls = Arc::new(AtomicUsize::new(0));
        let supervisor = Supervisor::new(Box::new(TimesOut {
            delay: Duration::from_millis(400),
            calls: Arc::clone(&calls),
        }));
        let drain = || Request::Drain {
            instance_id: "a".into(),
            from_seq: 0,
        };

        // The second call queues behind the first, which times out.
        let first = {
            let supervisor = supervisor.clone();
            thread::spawn(move || supervisor.call(drain()))
        };
        thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        let second = supervisor.call(Request::List);
        let waited = started.elapsed();

        assert!(first.join().unwrap().is_err());
        assert!(matches!(&second, Err(error) if errors::supervisor_link_lost(error)));
        // It failed when the first did, and never wrote to the stream.
        assert!(waited < Duration::from_millis(600), "{waited:?}");
        let started = Instant::now();
        assert!(supervisor.call(Request::List).is_err());
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(supervisor.is_broken());
    }

    #[test]
    fn a_call_that_panicked_leaves_a_handle_that_only_fails() {
        struct Panics;
        impl SupervisorPort for Panics {
            fn request(&mut self, _: Request) -> soyeht_ptyd::Result<Response> {
                panic!("the port failed")
            }
            fn supervisor_id(&self) -> &str {
                "panics"
            }
            fn supervisor_generation(&self) -> u32 {
                1
            }
        }
        let supervisor = Supervisor::new(Box::new(Panics));
        let caller = supervisor.clone();
        assert!(
            thread::spawn(move || caller.call(Request::List))
                .join()
                .is_err()
        );

        let again = supervisor.call(Request::List);

        assert!(matches!(&again, Err(error) if errors::supervisor_link_lost(error)));
        assert!(supervisor.is_broken());
    }

    #[test]
    fn the_tick_drops_a_broken_link_at_once_and_a_request_under_the_lock_fails_fast() {
        let (mut core, root, _) = open_core();
        let calls = Arc::new(AtomicUsize::new(0));
        core.link_supervisor(Box::new(TimesOut {
            delay: Duration::ZERO,
            calls: Arc::clone(&calls),
        }));
        let link = core.supervisor().unwrap();
        assert!(link.call(Request::List).is_err());

        // Nothing has settled the failed call yet: the tick does not wait for it to.
        let started = Instant::now();
        let outcome = core.pty_request(Request::List);

        assert!(outcome.is_err());
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(core.ptyd.is_none());

        // A link that a job broke, and the tick has not met yet, is dropped before anything
        // else uses it.
        core.link_supervisor(Box::new(TimesOut {
            delay: Duration::ZERO,
            calls: Arc::new(AtomicUsize::new(0)),
        }));
        assert!(core.supervisor().unwrap().call(Request::List).is_err());
        assert!(core.ptyd.is_some());
        core.ensure_supervisor();
        assert!(core.ptyd.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The pane of `open_core` and its process.
    const INSTANCE: &str = "shell-instance";

    fn type_into(core: &mut Core, ids: &str, key: &str, bytes: &str) -> crate::offload::Offload {
        let (connection, pane) = ids.split_once('|').unwrap();
        let input = core.dispatch(
            connection,
            request(
                9,
                "pane.input",
                json!({"paneId":pane,"bytes":bytes,"idempotencyKey":key}),
            ),
        );
        let Ok(Outcome::Offload(offload)) = input else {
            panic!("pane.input did not offload")
        };
        offload
    }

    #[test]
    fn an_input_follows_the_sends_to_its_process_that_were_made_before_it() {
        let (mut core, root, ids) = open_core();
        let (release, gate) = mpsc::channel();
        let (mut stub, _, asked) = Stub::new(Duration::ZERO);
        stub.gate = Some(Mutex::new(gate));
        core.link_supervisor(Box::new(stub));
        let resize = |cols| Request::Resize {
            instance_id: INSTANCE.into(),
            cols,
            rows: 24,
        };
        core.send_later(resize(80));
        let deadline = Instant::now() + Duration::from_secs(30);
        while asked.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "the size was never sent");
            thread::sleep(Duration::from_millis(1));
        }
        // The first size is on its way; a second waits in the queue.
        core.send_later(resize(90));

        let (_, work, _) = type_into(&mut core, &ids, "typed", "bHMK").into_parts();
        let typing = thread::spawn(move || drop(work()));
        thread::sleep(Duration::from_millis(150));
        // The typed bytes wait for the size before them, though nothing else holds the stream.
        assert_eq!(*asked.lock().unwrap(), ["shell-instance:80"]);
        release.send(()).unwrap();
        release.send(()).unwrap();
        typing.join().unwrap();

        assert_eq!(
            *asked.lock().unwrap(),
            ["shell-instance:80", "shell-instance:90", "input:bHMK"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inputs_to_one_process_reach_it_in_the_order_they_were_dispatched() {
        let (mut core, root, ids) = open_core();
        let (stub, _, asked) = Stub::new(Duration::ZERO);
        core.link_supervisor(Box::new(stub));
        let (_, first, _) = type_into(&mut core, &ids, "first", "YQ==").into_parts();
        let (_, second, _) = type_into(&mut core, &ids, "second", "Yg==").into_parts();

        // The later one gets to its work first.
        let later = thread::spawn(move || drop(second()));
        thread::sleep(Duration::from_millis(150));
        assert!(asked.lock().unwrap().is_empty());
        drop(first());
        later.join().unwrap();

        assert_eq!(*asked.lock().unwrap(), ["input:YQ==", "input:Yg=="]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_turn_that_never_ran_does_not_hold_the_ones_after_it() {
        let (mut core, root, _) = open_core();
        let abandoned = core.turn(&[INSTANCE]);
        let next = core.turn(&[INSTANCE]);

        drop(abandoned);
        let started = Instant::now();
        next.wait();

        assert!(started.elapsed() < Duration::from_millis(100));
        std::fs::remove_dir_all(root).unwrap();
    }
}
