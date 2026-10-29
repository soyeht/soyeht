//! The engine's link to the process supervisor: when a lost link is retried, and what
//! becomes of a pane the supervisor refuses to start again.
//!
//! Reconnecting runs under the Core lock (ADR 0079), so a supervisor that cannot be reached
//! or used is retried with a growing delay instead of on every tick, and one pane that cannot
//! be restarted never keeps the whole link from being established.

use crate::{
    core::Core,
    errors::{self, Error, Result},
    streak::Streak,
};
use soyeht_platform::{
    log::{self, Failure as _, Line},
    process::detached::spawn_service,
};
use soyeht_protocol::{
    PaneState,
    profile::{PTYD_LOG, PTYD_SOCKET},
};
use soyeht_ptyd::wire_kind;
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
        if self.ptyd.is_some() {
            self.settle_link(now);
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
        match self.connect_supervisor() {
            Ok(()) => {
                // The reconciliation that made the link starts every pane that has no process.
                self.supervisor_retry.spawn_retries.clear();
                self.supervisor_retry.close_retries.clear();
                self.supervisor_retry.connected_ms = Some(now);
                Link::Up
            }
            Err(error) => {
                if self.supervisor_retry.streak.fail(&error.failure_kind()) {
                    log::failure("engine.supervisor_unavailable", &error);
                }
                self.supervisor_retry.due_ms =
                    now.saturating_add(self.supervisor_retry.streak.next_delay_ms());
                self.start_supervisor(now);
                Link::Down
            }
        }
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
pub(crate) mod tests {
    use super::*;
    use crate::{errors, ports::Clock, ports::testing::FakeSupervisor};
    use soyeht_platform::local::{Listener, connect};
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
            let listener = Listener::bind(&socket).unwrap();
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
        fn request(
            &mut self,
            _request: soyeht_ptyd::wire::Request,
        ) -> soyeht_ptyd::Result<Response> {
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
        let spawned = core.spawn(&id);
        assert!(spawned.is_err());
        core.settle_spawn(&id, spawned);

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
    fn serve_registry(directory: &std::path::Path) {
        use soyeht_protocol::frame::read_json;
        use soyeht_ptyd::wire::Request;
        let listener = Listener::bind(&directory.join(PTYD_SOCKET)).unwrap();
        std::thread::spawn(move || {
            while let Ok(mut stream) = listener.accept() {
                while let Ok(Some(request)) = read_json::<Request>(&mut stream) {
                    let response = match request {
                        Request::Hello { .. } => Response::Welcome {
                            version: VERSION,
                            supervisor_id: "served".into(),
                            supervisor_generation: 2,
                        },
                        Request::PendingInputs => Response::InputReceipts(Vec::new()),
                        Request::List => Response::Instances(Vec::new()),
                        _ => Response::Ok,
                    };
                    if write_json(&mut stream, &response).is_err() {
                        break;
                    }
                }
            }
        });
    }

    #[test]
    fn a_lost_link_is_dropped_once_and_then_made_again() {
        let (mut core, directory, id, calls) = core_with_unstarted_pane(Vec::new());
        let spawned = core.spawn(&id);
        assert!(spawned.is_err());
        core.settle_spawn(&id, spawned);

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
}
