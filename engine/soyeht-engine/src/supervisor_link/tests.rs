use super::*;
use crate::{errors, ports::Clock, ports::testing::FakeSupervisor};
use soyeht_platform::local::{ProvedListener, connect};
use soyeht_protocol::{frame::write_json, profile::PTYD_SOCKET};
use soyeht_ptyd::wire::{Response, VERSION};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
};

/// A supervisor socket that accepts every connection and hangs up, after a welcome when
/// `welcome` is set (so the connection works and the reconciliation then fails).
struct HangUp {
    accepted: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    socket: std::path::PathBuf,
}

impl HangUp {
    fn start(directory: &std::path::Path, welcome: bool) -> Self {
        let socket = directory.join(PTYD_SOCKET);
        let listener = ProvedListener::bind(&socket).unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (count, done) = (Arc::clone(&accepted), Arc::clone(&stop));
        std::thread::spawn(move || {
            while let Ok(mut stream) = listener.accept() {
                if done.load(Ordering::SeqCst) {
                    break;
                }
                count.fetch_add(1, Ordering::SeqCst);
                if welcome {
                    let hello = soyeht_protocol::frame::Frame::read_from(&mut stream);
                    assert!(hello.is_ok());
                    let welcome = Response::Welcome {
                        version: VERSION,
                        supervisor_id: "test".into(),
                        supervisor_generation: 1,
                        features: Vec::new(),
                    };
                    assert!(write_json(&mut stream, &welcome).is_ok());
                }
                drop(stream);
            }
        });
        Self {
            accepted,
            stop,
            socket,
        }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for HangUp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread so it sees the flag and ends.
        drop(connect(&self.socket));
    }
}

/// A clock the test moves.
#[derive(Debug)]
pub(crate) struct Manual(pub(crate) AtomicI64);

impl Clock for Manual {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(crate) fn short_directory() -> std::path::PathBuf {
    // A socket path has a short limit: keep the folder name short.
    let directory = std::env::temp_dir().join(format!(
        "sn-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// 500 ticks against a supervisor that fails every attempt reach it at most once, and the
/// tick itself never reports the failure (the retry streak owns the log line).
fn assert_one_attempt_per_burst(welcome: bool) {
    let directory = short_directory();
    let supervisor = HangUp::start(&directory, welcome);
    let mut core = Core::open(&directory).unwrap();
    // The burst happens at one instant: a loaded host that slows the ticks past the retry
    // delay must not turn into a second attempt.
    core.clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
    // Each attempt is made and settled in its tick, so the burst counts every one.
    core.background = crate::offload::Background::inline();
    let opened = supervisor.accepted();
    for _ in 0..500 {
        assert!(core.poll_output().is_ok());
    }
    // The retry delay starts at 200 ms: a burst of ticks reaches the supervisor once.
    let attempts = supervisor.accepted() - opened;
    assert!(attempts <= 1, "{attempts} connection attempts in one burst");
    assert!(core.ptyd.is_none());
}

#[test]
fn an_unreachable_supervisor_is_not_retried_on_every_tick() {
    assert_one_attempt_per_burst(false);
}

#[test]
fn a_supervisor_that_welcomes_and_drops_is_not_retried_on_every_tick() {
    assert_one_attempt_per_burst(true);
}

#[test]
fn a_link_lost_soon_after_it_was_made_keeps_the_delay_and_a_stable_one_resets_it() {
    let directory = short_directory();
    let mut core = Core::open(&directory).unwrap();
    let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
    core.clock = clock.clone();
    // The connection job runs and applies in the caller, for determinism.
    core.background = crate::offload::Background::inline();
    core.supervisor_retry.connected_ms = Some(999_000);
    assert_eq!(core.ensure_supervisor(), Link::Down);
    assert_eq!(core.supervisor_retry.streak.attempts(), 1);
    assert!(core.supervisor_retry.due_ms > 1_000_000);

    // Up for longer than the stable time: the next loss starts over.
    core.supervisor_retry.due_ms = 0;
    core.supervisor_retry.connected_ms = Some(1_000_000 - STABLE_MS);
    assert_eq!(core.ensure_supervisor(), Link::Down);
    assert_eq!(core.supervisor_retry.streak.attempts(), 1);
    assert!(core.supervisor_retry.due_ms > 1_000_000);
    assert!(core.supervisor_retry.connected_ms.is_none());
}

/// A program that records each start of it: one line in `started` of the folder it is in.
#[cfg(unix)]
fn recording_program(directory: &std::path::Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let program = directory.join("fake-ptyd");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\necho started >> '{}'\n",
            directory.join("started").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

#[cfg(unix)]
fn starts(directory: &std::path::Path) -> usize {
    std::fs::read_to_string(directory.join("started")).map_or(0, |text| text.lines().count())
}

#[cfg(unix)]
fn wait_for_starts(directory: &std::path::Path, count: usize) {
    for _ in 0..200 {
        if starts(directory) >= count {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{} starts, wanted {count}", starts(directory));
}

/// A core whose clock the test moves and whose background work runs on the spot.
#[cfg(unix)]
fn core_with_clock(directory: &std::path::Path) -> (Core, Arc<Manual>) {
    let mut core = Core::open(directory).unwrap();
    let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
    core.clock = clock.clone();
    core.background = crate::offload::Background::inline();
    (core, clock)
}

#[cfg(unix)]
#[test]
fn a_supervisor_that_is_gone_is_started_again_once_for_each_grace_period() {
    let directory = short_directory();
    let (mut core, clock) = core_with_clock(&directory);
    core.restart_supervisor_with(recording_program(&directory));
    assert_eq!(core.ensure_supervisor(), Link::Down);
    wait_for_starts(&directory, 1);
    // Ticks inside the grace period, and the retry delays that pass inside it, start nothing.
    for _ in 0..20 {
        clock.0.fetch_add(500, Ordering::SeqCst);
        assert_eq!(core.ensure_supervisor(), Link::Down);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(starts(&directory), 1);
    // Past it, a supervisor that is still gone is started again.
    clock.0.fetch_add(START_GRACE_MS, Ordering::SeqCst);
    assert_eq!(core.ensure_supervisor(), Link::Down);
    wait_for_starts(&directory, 2);
}

#[cfg(unix)]
#[test]
fn a_program_that_cannot_be_started_is_one_streak_not_a_failure_for_each_try() {
    let directory = short_directory();
    let (mut core, clock) = core_with_clock(&directory);
    core.restart_supervisor_with(directory.join("no-such-program"));
    for tries in 1..=3 {
        assert_eq!(core.ensure_supervisor(), Link::Down);
        assert_eq!(core.supervisor_retry.start_streak.attempts(), tries);
        clock.0.fetch_add(START_GRACE_MS, Ordering::SeqCst);
    }
    // Once it can be started the streak ends.
    core.restart_supervisor_with(recording_program(&directory));
    assert_eq!(core.ensure_supervisor(), Link::Down);
    wait_for_starts(&directory, 1);
    assert_eq!(core.supervisor_retry.start_streak.attempts(), 0);
}

#[cfg(unix)]
#[test]
fn without_a_program_to_start_the_engine_only_reconnects() {
    let directory = short_directory();
    let (mut core, clock) = core_with_clock(&directory);
    for _ in 0..5 {
        clock.0.fetch_add(START_GRACE_MS, Ordering::SeqCst);
        assert_eq!(core.ensure_supervisor(), Link::Down);
    }
    assert_eq!(starts(&directory), 0);
    assert!(core.supervisor_retry.program.is_none());
}

#[cfg(unix)]
#[test]
fn a_supervisor_that_answers_is_not_started_again() {
    let directory = short_directory();
    // It accepts and hangs up, so the link cannot be made but something listens.
    let supervisor = HangUp::start(&directory, false);
    let (mut core, _clock) = core_with_clock(&directory);
    core.restart_supervisor_with(recording_program(&directory));
    let opened = supervisor.accepted();
    assert_eq!(core.ensure_supervisor(), Link::Down);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(supervisor.accepted() > opened);
    assert_eq!(starts(&directory), 0);
}

pub(crate) fn remote(kind: &str) -> Error {
    Error::Supervisor(soyeht_ptyd::Error::Remote {
        kind: kind.into(),
        message: "x".into(),
    })
}

#[test]
fn only_a_refused_spawn_stops_a_pane() {
    let directory = short_directory();
    let mut core = Core::open(&directory).unwrap();
    core.link_supervisor(Box::new(FakeSupervisor::default()));
    assert!(core.refused_by_supervisor(&remote(wire_kind::SPAWN_REFUSED)));
    core.ptyd = None;
    assert!(!core.refused_by_supervisor(&remote(wire_kind::SPAWN_REFUSED)));
    core.link_supervisor(Box::new(FakeSupervisor::default()));
    for kind in [
        // A spawn that failed for lack of a resource or time may succeed on the next try.
        wire_kind::SPAWN_FAILED,
        wire_kind::OPERATION_FAILED,
        wire_kind::NOT_FOUND,
        wire_kind::STALE_ENGINE_GENERATION,
    ] {
        assert!(!core.refused_by_supervisor(&remote(kind)), "{kind}");
        // The pane is left as it is and the reconciliation fails, to be retried.
        assert!(core.stop_unrestartable_pane("pane", remote(kind)).is_err());
    }
    assert!(!core.refused_by_supervisor(&errors::internal("x")));
}

/// A supervisor the test scripts: it answers from `responses`, then fails as a lost link
/// does, and counts what it was asked.
pub(crate) struct Scripted {
    pub(crate) responses: std::collections::VecDeque<soyeht_ptyd::Result<Response>>,
    pub(crate) calls: Arc<AtomicUsize>,
}

impl crate::ports::SupervisorPort for Scripted {
    fn request(&mut self, _request: soyeht_ptyd::wire::Request) -> soyeht_ptyd::Result<Response> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .pop_front()
            .unwrap_or(Err(soyeht_ptyd::Error::InvalidResponse))
    }

    fn supervisor_id(&self) -> &str {
        "scripted"
    }

    fn supervisor_generation(&self) -> u32 {
        1
    }
}

/// A core with a pane that has no process yet, linked to a supervisor that gives
/// `responses`; the second value counts the supervisor's requests.
pub(crate) fn core_with_unstarted_pane(
    responses: Vec<soyeht_ptyd::Result<Response>>,
) -> (Core, std::path::PathBuf, String, Arc<AtomicUsize>) {
    let directory = short_directory();
    let mut core = Core::open(&directory).unwrap();
    // Closes end in the caller, so the call that settles one has run when it returns.
    core.background = crate::offload::Background::inline();
    let id = crate::test_panes::insert_terminal_pane(&mut core, &directory, "p", false);
    let pane = core.panes.get_mut(&id).unwrap();
    pane.stored.instance_id = None;
    pane.stored.supervisor_id = None;
    let calls = Arc::new(AtomicUsize::new(0));
    core.link_supervisor(Box::new(Scripted {
        responses: responses.into(),
        calls: Arc::clone(&calls),
    }));
    (core, directory, id, calls)
}

pub(crate) fn events_named(core: &mut Core, name: &str) -> usize {
    core.store
        .replay(0)
        .unwrap()
        .1
        .iter()
        .filter(|event| event.method == name)
        .count()
}

#[test]
fn a_refused_spawn_keeps_the_link_stops_the_pane_and_reconciles_nothing() {
    let (mut core, _directory, id, calls) =
        core_with_unstarted_pane(vec![Err(soyeht_ptyd::Error::Remote {
            kind: wire_kind::SPAWN_REFUSED.into(),
            message: "the folder is missing".into(),
        })]);
    core.spawn(&id).unwrap();

    assert!(core.ptyd.is_some());
    assert_eq!(core.panes[&id].stored.pane.state, PaneState::Dead);
    assert_eq!(core.supervisor_retry.spawn_retries.attempts(&id), 0);
    // The tick finds the link up: it neither reconnects nor reconciles every pane.
    assert_eq!(core.ensure_supervisor(), Link::Up);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        events_named(&mut core, soyeht_protocol::event::PTYD_LOST),
        0
    );
    assert_eq!(
        events_named(&mut core, soyeht_protocol::event::PTYD_RECONNECTED),
        0
    );
}

#[test]
fn a_failure_the_supervisor_reports_or_the_store_raises_keeps_the_link_and_is_one_streak() {
    let (mut core, _directory, id, _calls) = core_with_unstarted_pane(Vec::new());
    let event = "engine.pane_spawn_failed";
    let storage = || errors::storage(soyeht_store::database::Error::TestFixture("full"));
    for kind in [wire_kind::SPAWN_FAILED, wire_kind::OPERATION_FAILED] {
        core.settle_spawn(&id, Err(remote(kind)));
        assert!(core.ptyd.is_some(), "{kind}");
    }
    core.settle_spawn(&id, Err(storage()));
    core.settle_supervisor_call("engine.pane_close_failed", Err(storage()));
    assert!(core.ptyd.is_some());
    // The attempts count the consecutive failures of one action; a success ends them.
    assert_eq!(core.supervisor_retry.call_failures[event].attempts(), 3);
    core.settle_spawn(&id, Ok(()));
    assert_eq!(core.supervisor_retry.call_failures[event].attempts(), 0);
    assert_eq!(
        events_named(&mut core, soyeht_protocol::event::PTYD_LOST),
        0
    );
    // A non-refusal leaves the pane to the tick's retry (see `spawn_retry`).
    assert_ne!(core.panes[&id].stored.pane.state, PaneState::Dead);
}

/// A supervisor on the socket that welcomes the engine and answers its registry reads.
pub(crate) fn serve_registry(directory: &std::path::Path) {
    use {soyeht_protocol::frame::read_json, soyeht_ptyd::wire::Request};
    let listener = ProvedListener::bind(&directory.join(PTYD_SOCKET)).unwrap();
    std::thread::spawn(move || {
        while let Ok(mut stream) = listener.accept() {
            std::thread::spawn(move || {
                while let Ok(Some(request)) = read_json::<Request>(&mut stream) {
                    let response = match request {
                        Request::Hello { .. } => Response::Welcome {
                            version: VERSION,
                            supervisor_id: "served".into(),
                            supervisor_generation: 2,
                            features: Vec::new(),
                        },
                        Request::PendingInputs => Response::InputReceipts(Vec::new()),
                        Request::List => Response::Instances(Vec::new()),
                        _ => Response::Ok,
                    };
                    if write_json(&mut stream, &response).is_err() {
                        break;
                    }
                }
            });
        }
    });
}

#[test]
fn a_lost_link_is_dropped_once_and_then_made_again() {
    let (mut core, directory, id, calls) = core_with_unstarted_pane(Vec::new());
    core.spawn(&id).unwrap();

    assert!(core.ptyd.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // `pty_request` recorded the loss; settling the same failure does not record it again.
    assert_eq!(
        events_named(&mut core, soyeht_protocol::event::PTYD_LOST),
        1
    );

    // The registry the engine reconciles is empty: this test is about the link.
    core.panes.remove(&id);
    serve_registry(&directory);
    core.background = crate::offload::Background::inline();
    assert_eq!(core.ensure_supervisor(), Link::Up);
    assert!(core.ptyd.is_some());
    assert_eq!(
        events_named(&mut core, soyeht_protocol::event::PTYD_RECONNECTED),
        1
    );
    assert_eq!(
        events_named(&mut core, soyeht_protocol::event::PTYD_LOST),
        1
    );
}

#[test]
fn only_a_transport_failure_says_the_link_is_lost() {
    assert!(errors::supervisor_link_lost(&Error::Supervisor(
        soyeht_ptyd::Error::InvalidResponse
    )));
    assert!(!errors::supervisor_link_lost(&remote(
        wire_kind::SPAWN_REFUSED
    )));
    assert!(!errors::supervisor_link_lost(&errors::storage(
        soyeht_store::database::Error::TestFixture("full")
    )));
    assert!(!errors::supervisor_link_lost(&errors::internal("x")));
}
