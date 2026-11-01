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
    /// A drain fails like a lost link, after its delay.
    fail: bool,
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
            fail: false,
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
                if self.fail {
                    return Err(soyeht_ptyd::Error::InvalidResponse);
                }
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
            Request::Close { instance_id } => {
                asked.push(format!("close:{instance_id}"));
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

/// Waits until `ready`, failing with `what` after 30 s.
fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(1));
    }
}

fn resize_of(instance: &str, cols: u16) -> Request {
    Request::Resize {
        instance_id: instance.into(),
        cols,
        rows: 24,
    }
}

/// The core, its folder, the ids of its pane, what lets the held resizes go, and what the
/// supervisor was asked (see `core_with_a_size_on_its_way`).
type SizeOnItsWay = (
    Core,
    std::path::PathBuf,
    String,
    mpsc::Sender<()>,
    Arc<Mutex<Vec<String>>>,
);

/// A core whose supervisor holds every resize until it is let go by `release`, with the size 80
/// of `instance` already on its way. `asked` says what the supervisor was asked.
fn core_with_a_size_on_its_way(instance: &str) -> SizeOnItsWay {
    let (mut core, root, ids) = open_core();
    let (release, gate) = mpsc::channel();
    let (mut stub, _, asked) = Stub::new(Duration::ZERO);
    stub.gate = Some(Mutex::new(gate));
    core.link_supervisor(Box::new(stub));
    core.send_later(resize_of(instance, 80));
    wait_for("the size was never sent", || {
        !asked.lock().unwrap().is_empty()
    });
    (core, root, ids, release, asked)
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
    wait_for("the supervisor was never asked", || {
        draining.load(Ordering::SeqCst)
    });

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
    let (mut core, root, ids, release, asked) = core_with_a_size_on_its_way(INSTANCE);
    // The first size is on its way; a second waits in the queue.
    core.send_later(resize_of(INSTANCE, 90));

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

#[test]
fn typing_does_not_wait_for_a_free_worker() {
    let (mut core, root, ids) = open_core();
    let (stub, _, asked) = Stub::new(Duration::ZERO);
    core.link_supervisor(Box::new(stub));
    // Every shared worker is busy with something that waits.
    let (release, gate) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    for index in 0..8 {
        let gate = Arc::clone(&gate);
        core.spawn_background(
            format!("test.busy:{index}"),
            move || {
                gate.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(30))
                    .ok();
                Ok(())
            },
            |_, _| Ok(()),
        )
        .unwrap();
    }
    core.send_later(Request::Resize {
        instance_id: INSTANCE.into(),
        cols: 80,
        rows: 24,
    });
    let (_, work, _) = type_into(&mut core, &ids, "typed", "bHMK").into_parts();

    let started = Instant::now();
    drop(work());

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(*asked.lock().unwrap(), ["shell-instance:80", "input:bHMK"]);
    for _ in 0..8 {
        release.send(()).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sends_an_input_took_come_back_when_its_request_ran_out_of_time_first() {
    let (mut core, root, ids, release, asked) = core_with_a_size_on_its_way(INSTANCE);
    core.send_later(resize_of(INSTANCE, 90));
    let (_, work, expire) = type_into(&mut core, &ids, "typed", "bHMK").into_parts();
    assert!(core.supervisor_later.is_empty());
    let waiting = thread::spawn(move || drop(work()));
    thread::sleep(Duration::from_millis(100));

    // The request is answered as late before its work got its turn.
    drop(expire(&mut core));
    assert!(matches!(
        core.supervisor_later.as_slice(),
        [Request::Resize { cols: 90, .. }]
    ));
    release.send(()).unwrap();
    waiting.join().unwrap();

    // Its work sent nothing: neither the size it had taken nor the typed bytes.
    assert_eq!(*asked.lock().unwrap(), ["shell-instance:80"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_read_on_a_replaced_link_does_not_cool_the_pane_down() {
    let (mut core, root, ids) = open_core();
    let pane = ids.split_once('|').unwrap().1.to_owned();
    let (mut stub, draining, _) = Stub::new(Duration::from_millis(300));
    stub.fail = true;
    core.link_supervisor(Box::new(stub));
    core.poll_output().unwrap();
    wait_for("the supervisor was never asked", || {
        draining.load(Ordering::SeqCst)
    });

    // The link is replaced while the read is on its way, then the read fails.
    core.link_supervisor(Box::new(FakeSupervisor::default()));
    let job = format!("pane.drain:{pane}");
    wait_for("the read was never applied", || {
        core.apply_background();
        !core.background.is_active(&job)
    });

    assert!(core.ptyd.is_some());
    assert!(core.background.accepts(&job));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_close_goes_after_the_sends_ahead_of_it_and_the_ones_queued_for_the_process_are_dropped() {
    let (mut core, root, _, release, asked) = core_with_a_size_on_its_way("a");
    let ack = |seq| Request::Ack {
        instance_id: "a".into(),
        seq,
    };
    // Behind the size on its way: a size and two acknowledgements for "a", and one for "b".
    core.send_later(resize_of("a", 90));
    core.send_later(ack(1));
    core.send_later(ack(2));
    core.send_later(Request::Ack {
        instance_id: "b".into(),
        seq: 1,
    });
    // The later acknowledgement of a process replaces the one not yet sent.
    assert_eq!(core.supervisor_later.len(), 3);

    core.close_process("a");

    // Nothing queued for "a" is left to find it closed; "b" keeps its acknowledgement.
    assert_eq!(core.supervisor_later.len(), 1);
    // The close waits for the size on its way.
    thread::sleep(Duration::from_millis(100));
    assert_eq!(*asked.lock().unwrap(), ["a:80"]);
    release.send(()).unwrap();
    wait_for("the close was never sent", || {
        asked.lock().unwrap().contains(&"close:a".to_owned())
    });
    assert_eq!(*asked.lock().unwrap(), ["a:80", "close:a"]);
    while core.background.is_active("close:a") || core.background.is_active(LATER_JOB) {
        core.apply_background();
        thread::sleep(Duration::from_millis(1));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_input_job_waits_for_the_supervisor_off_the_lock_after_the_sends_ahead_of_it() {
    let (mut core, root, _) = open_core();
    core.background = crate::offload::Background::default();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&asked);
    core.link_supervisor(Box::new(crate::test_panes::Answering(move |request| {
        Ok(match request {
            Request::Resize { cols, .. } => {
                record.lock().unwrap().push(format!("size:{cols}"));
                Response::Ok
            }
            Request::Input { bytes, .. } => {
                // A supervisor that takes its time to take the bytes.
                thread::sleep(Duration::from_millis(400));
                record.lock().unwrap().push(format!("input:{bytes}"));
                Response::Accepted { bytes: 4 }
            }
            _ => Response::Ok,
        })
    })));
    core.send_later(resize_of(INSTANCE, 90));
    let accepted = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&accepted);

    let started = Instant::now();
    core.start_input(
        "input-job".into(),
        INSTANCE,
        Request::Input {
            instance_id: INSTANCE.into(),
            bytes: "bHMK".into(),
            operation_key: "operation".into(),
            fingerprint: "fingerprint".into(),
        },
        move |_, bytes| {
            *seen.lock().unwrap() = Some(bytes.unwrap());
            Ok(())
        },
    )
    .unwrap();
    // The same job asked for again while it runs sends nothing.
    core.start_input(
        "input-job".into(),
        INSTANCE,
        Request::Input {
            instance_id: INSTANCE.into(),
            bytes: "again".into(),
            operation_key: "operation".into(),
            fingerprint: "fingerprint".into(),
        },
        |_, _| Ok(()),
    )
    .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "the input waited under the lock: {:?}",
        started.elapsed()
    );

    crate::test_panes::apply_until_done(&mut core, "input-job");
    assert_eq!(*accepted.lock().unwrap(), Some(Some(4)));
    // The size queued for the process went in front of the bytes.
    assert_eq!(*asked.lock().unwrap(), ["size:90", "input:bHMK"]);
    std::fs::remove_dir_all(root).unwrap();
}
