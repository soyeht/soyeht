//! The engine's link to the supervisor, as a handle that waiting borrows off the Core lock
//! (ADR 0116).
//!
//! A request to the supervisor waits for its answer for up to [`IO_TIMEOUT`], and the terminal
//! output path shares the Core lock with every request, so a request that waits under the lock
//! stalls every terminal while the supervisor is slow. The link is therefore a [`Supervisor`]:
//! a cheap clone that owns the connection behind a mutex of its own. Work copies its inputs
//! under the lock, calls through its clone off the lock (`Outcome::Offload` for a request,
//! `Core::spawn_background` for the engine's own work), and settles the answer under the lock
//! with [`Core::settle_supervisor`].
//!
//! The mutex of the connection only orders the callers of this one connection, one request and
//! answer at a time; the Core lock is never held while waiting for it by the paths that were
//! moved. What still waits under the Core lock is counted by `tests/blocking_io_rule.rs`.

use crate::{
    core::Core,
    errors::{self, Result},
    offload::SharedPort,
    ports::{SupervisorPort, call_supervisor},
};
use soyeht_ptyd::{
    client::IO_TIMEOUT,
    wire::{Request, Response},
};
use std::{sync::Arc, time::Duration};

/// How long a request answered off the lock may take, from the start of its work: the wait
/// behind the requests already using the connection, then its own write and read.
pub(crate) const DEADLINE: Duration = IO_TIMEOUT.saturating_mul(6);

/// The background job that sends the follow-ups nobody waits for.
const LATER_JOB: &str = "supervisor.later";
/// The log event of a follow-up that failed.
const LATER_FAILED: &str = "engine.supervisor_follow_up_failed";

/// The connection to the supervisor. Clones share it.
#[derive(Clone)]
pub struct Supervisor {
    port: SharedPort<dyn SupervisorPort>,
    id: Arc<str>,
    generation: u32,
}

impl Supervisor {
    pub fn new(port: Box<dyn SupervisorPort>) -> Self {
        Self {
            id: Arc::from(port.supervisor_id()),
            generation: port.supervisor_generation(),
            port: SharedPort::new(port),
        }
    }

    pub fn supervisor_id(&self) -> &str {
        &self.id
    }

    pub fn supervisor_generation(&self) -> u32 {
        self.generation
    }

    /// Sends one request and waits for the answer. The wait is bounded by [`IO_TIMEOUT`] for
    /// the request itself, plus the wait for the callers ahead of it. Never call it under the
    /// Core lock from a path that was moved off it.
    pub fn call(&self, request: Request) -> Result<Response> {
        self.port.with(|port| call_supervisor(port, request))
    }

    /// Sends `batch` in order. A request the supervisor refuses does not stop the others; the
    /// first refusal is returned after them. A lost link stops the batch at once, since the
    /// rest cannot be sent.
    fn call_each(&self, batch: Vec<Request>) -> Result<()> {
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
        self.port.same_as(&other.port)
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
    /// (a call that outlived the link it started on says nothing about the new one). The caller
    /// still gets `ptyd_unavailable`.
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

    /// Sends `request` to the supervisor without waiting for it or its answer, for what the
    /// caller does not depend on (a confirmation, a size it already made durable). Requests go
    /// in the order given, and a size not yet sent is replaced by the newer one for the same
    /// process. A failure is logged under `engine.supervisor_follow_up_failed`.
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
        let used = supervisor.clone();
        let started = self.spawn_background(
            LATER_JOB.into(),
            move || supervisor.call_each(batch),
            move |core, sent| {
                let settled = core.settle_supervisor(&used, sent);
                core.settle_supervisor_call(LATER_FAILED, settled);
                Ok(())
            },
        );
        if let Err(error) = started {
            soyeht_platform::log::failure("engine.supervisor_follow_up_not_started", &error);
        }
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
            atomic::{AtomicBool, Ordering},
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
                    asked.push("input".into());
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
        assert_eq!(*asked.lock().unwrap(), ["drain", "input"]);
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
        assert_eq!(*asked.lock().unwrap(), ["input", "confirm"]);
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
}
