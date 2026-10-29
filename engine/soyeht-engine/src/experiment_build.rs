//! Experiment captures and the engine build they record (ADR 0079).
//!
//! Every capture records the SHA-256 of the engine's executable, which takes seconds to compute
//! on a loaded host. Nothing here waits for it under the Core lock. A caller under the lock asks
//! [`Core::capture_build`], which only reads it. When the hash is still running, the two things
//! that need it wait elsewhere:
//!
//! - Turning a capture on answers through `Outcome::Offload`: the wait runs off the lock, and
//!   the request then runs again under it, with the build known.
//! - Restoring the captures a restart left on does not hold up the engine's first listen. The
//!   wait runs as a background job, and its result restores them under the lock. Until then the
//!   output of those panes stays in the supervisor, so the capture still starts at the first
//!   byte after the restart. The wait is bounded by [`BUILD_WAIT`]: past it the captures stop
//!   with a reason and the panes drain again, since a hash that never finishes must not freeze
//!   a terminal. The supervisor keeps 8 MiB of output per pane, so a long wait loses the first
//!   bytes anyway.

use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::Offload,
};
use serde_json::Value;
use soyeht_pane_state::EngineBuild;
use soyeht_platform::log;
use soyeht_protocol::{PaneMode, method};
use std::time::{Duration, Instant};

/// How long a request to turn a capture on waits for the hash before it is refused.
const BUILD_WAIT: Duration = Duration::from_secs(60);
/// The background job that waits for the hash on behalf of the restored captures.
const RESTORE_JOB: &str = "experiment_capture.restore";

impl Core {
    /// The engine build, if it is known. It never waits, so it is safe under the Core lock;
    /// code that can come before the hash finishes checks [`Core::capture_build_ready`] first.
    pub(crate) fn capture_build(&self) -> Result<EngineBuild> {
        self.engine_build
            .ready()
            .unwrap_or_else(|| Err(errors::internal("The engine build is not known yet")))
    }

    /// Whether the hash has finished, with a build or with the failure that ended it.
    pub(crate) fn capture_build_ready(&self) -> bool {
        self.engine_build.ready().is_some()
    }

    /// `pane.experiment.set`. Turning a capture on while the engine build is still being hashed
    /// waits for it off the Core lock, then runs the request again. A request that cannot
    /// succeed is refused before it waits.
    pub(crate) fn pane_experiment_set(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Outcome> {
        if params["on"].as_bool() == Some(true) && !self.capture_build_ready() {
            let pane_id = errors::param_str(params, "paneId")?;
            let pane = self
                .panes
                .get(pane_id)
                .filter(|pane| !pane.stored.closed)
                .ok_or_else(|| errors::not_found("Pane not found"))?;
            if !matches!(pane.stored.pane.mode, PaneMode::Terminal | PaneMode::Agent) {
                return Err(errors::invalid(
                    "Experiment capture requires a terminal or agent pane",
                ));
            }
            let connection = connection.to_owned();
            let params = params.clone();
            return Ok(self.after_build(move |core, build| {
                build?;
                core.pane_experiment_set(&connection, &params)
            }));
        }
        self.pane_edit(connection, method::PANE_EXPERIMENT_SET, params)
            .map(Outcome::Reply)
    }

    /// `pane.experiment.hook`. A hook that arrives while the captures a restart left on still
    /// wait for the engine build would find its capture not running and be answered as a
    /// no-op, which its sender never repeats: its label would be lost. It waits for the build
    /// off the Core lock instead, settles those captures, and then runs as usual.
    pub(crate) fn pane_experiment_hook(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Outcome> {
        if self.capture_restore.is_empty() {
            return self
                .record_hook_event(connection, params)
                .map(Outcome::Reply);
        }
        let connection = connection.to_owned();
        let params = params.clone();
        Ok(self.after_build(move |core, build| {
            core.settle_restores_now(build);
            core.record_hook_event(&connection, &params)
                .map(Outcome::Reply)
        }))
    }

    /// Waits for the engine build off the Core lock, then runs `then` under it.
    fn after_build(
        &self,
        then: impl FnOnce(&mut Core, Result<EngineBuild>) -> Result<Outcome> + Send + 'static,
    ) -> Outcome {
        let cell = self.engine_build;
        Outcome::Offload(Offload::new(
            "The engine build is still being identified; try again",
            BUILD_WAIT,
            move || cell.wait(),
            then,
        ))
    }

    /// Settles, under the lock, the captures that wait for the build, once a caller has seen
    /// the hash finish before the background restore applied its result.
    fn settle_restores_now(&mut self, build: Result<EngineBuild>) {
        self.capture_restore_deadline = None;
        if let Err(error) = self.settle_waiting(build.map(|_| ())) {
            // The pane that failed is already stopped with its reason; the hook is answered
            // for its own pane.
            log::failure("engine.capture_restore_failed", &error);
        }
    }

    /// Starts the captures a restart left on, without waiting for the engine build.
    pub(crate) fn restore_experiment_captures(&mut self) -> Result<()> {
        let ids = self.captures_to_restore();
        if ids.is_empty() {
            return Ok(());
        }
        if let Some(build) = self.engine_build.ready() {
            return self.settle_restores(ids, build.map(|_| ()));
        }
        log::event(
            "engine.capture_restore_waiting",
            &[("panes", &ids.len().to_string())],
        );
        self.capture_restore.extend(ids);
        self.capture_restore_deadline = Some(Instant::now() + BUILD_WAIT);
        let cell = self.engine_build;
        self.spawn_background(
            RESTORE_JOB.into(),
            move || cell.wait(),
            Self::finish_capture_restore,
        )
    }

    /// Whether the output of `pane_id` is held back until its capture is restored.
    pub(crate) fn capture_restore_pending(&self, pane_id: &str) -> bool {
        self.capture_restore.contains(pane_id)
    }

    /// Gives up on the captures still waiting for the hash once [`BUILD_WAIT`] has passed, so
    /// a hash that never finishes cannot hold the output of their panes back for good.
    pub(crate) fn expire_capture_restore(&mut self) -> Result<()> {
        if self
            .capture_restore_deadline
            .is_none_or(|deadline| Instant::now() < deadline)
        {
            return Ok(());
        }
        self.capture_restore_deadline = None;
        let reason = errors::internal(format!(
            "The engine build was not known within {} s",
            BUILD_WAIT.as_secs()
        ));
        self.settle_waiting(Err(reason))
    }

    /// The panes whose capture is on and not running.
    fn captures_to_restore(&self) -> Vec<String> {
        self.panes
            .iter()
            .filter(|(id, pane)| {
                !pane.stored.closed
                    && matches!(pane.stored.pane.mode, PaneMode::Terminal | PaneMode::Agent)
                    && pane
                        .stored
                        .pane
                        .experiment_capture
                        .as_ref()
                        .is_some_and(|capture| capture.on)
                    && !self.terminal_captures.contains_key(*id)
                    && !self.agent_captures.contains_key(*id)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Restores, under the lock, the captures that waited for the hash.
    fn finish_capture_restore(&mut self, build: Result<EngineBuild>) -> Result<()> {
        self.capture_restore_deadline = None;
        self.settle_waiting(build.map(|_| ()))
    }

    /// Settles the panes that wait, whatever ended the wait. A pane that was closed, or whose
    /// capture was turned off or on again meanwhile, is left as it is.
    fn settle_waiting(&mut self, build: Result<()>) -> Result<()> {
        let waiting = std::mem::take(&mut self.capture_restore);
        let wanted = self.captures_to_restore();
        self.settle_restores(
            wanted
                .into_iter()
                .filter(|id| waiting.contains(id))
                .collect(),
            build,
        )
    }

    /// Starts the capture of each pane, or stops it with the reason the build is unknown. One
    /// pane that cannot be settled never keeps the others from it: its capture is dropped, the
    /// failure is logged, and the first one is returned at the end.
    fn settle_restores(&mut self, ids: Vec<String>, build: Result<()>) -> Result<()> {
        let mut first_failure = None;
        for id in ids {
            let settled = match &build {
                Ok(()) => self.restore_capture(&id),
                Err(error) => self.fail_experiment_capture(&id, Some(error.to_string())),
            };
            if let Err(error) = settled {
                self.terminal_captures.remove(&id);
                self.agent_captures.remove(&id);
                log::failure("engine.capture_restore_failed", &error);
                first_failure.get_or_insert(error);
            }
        }
        first_failure.map_or(Ok(()), Err)
    }

    fn restore_capture(&mut self, id: &str) -> Result<()> {
        let restored = match self.panes[id].stored.pane.mode {
            PaneMode::Terminal => self.prepare_terminal_capture(id).map(|capture| {
                self.terminal_captures.insert(id.to_owned(), capture);
            }),
            PaneMode::Agent => self.prepare_agent_capture(id).map(|capture| {
                self.agent_captures.insert(id.to_owned(), capture);
            }),
            _ => return Ok(()),
        };
        match restored {
            Ok(()) => Ok(()),
            Err(error) => self.fail_experiment_capture(id, Some(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_panes::replay_rows;
    use crate::{
        engine_build::EngineBuildCell,
        outbox,
        ports::{
            SupervisorPort,
            testing::{EmptyPortScan, FixedClock},
        },
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::json;
    use soyeht_pane_state::RecordedRow;
    use soyeht_protocol::PaneExperimentCapture;
    use soyeht_ptyd::wire::{Output, Request, Response};
    use std::{
        fs,
        path::Path,
        sync::{Arc, Condvar, Mutex},
        thread,
        time::Instant,
    };
    use uuid::Uuid;

    const PANE: &str = "capture-pane";

    /// A hash that finishes when the test says so, standing in for a slow disk.
    struct Gate {
        open: Mutex<bool>,
        wake: Condvar,
    }

    impl Gate {
        const fn new() -> Self {
            Self {
                open: Mutex::new(false),
                wake: Condvar::new(),
            }
        }

        fn release(&self) {
            *self.open.lock().unwrap() = true;
            self.wake.notify_all();
        }

        /// Returns the build once released. A test that never releases it gets an error rather
        /// than a hang, so a request that waited for it fails the test.
        fn hash(&self) -> Result<EngineBuild> {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut open = self.open.lock().unwrap();
            while !*open {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(errors::internal("the test never finished the hash"));
                }
                open = self.wake.wait_timeout(open, left).unwrap().0;
            }
            Ok(EngineBuild {
                package_version: "0.1.0".into(),
                executable_sha256: "a".repeat(64),
            })
        }
    }

    fn terminal_core(root: &Path, capture: bool, cell: &'static EngineBuildCell) -> Core {
        fs::create_dir(root).unwrap();
        let mut core = Core::open(root).unwrap();
        core.engine_build = cell;
        core.clock = Arc::new(FixedClock(0));
        core.web = crate::web::Service::new(Box::new(EmptyPortScan));
        crate::test_panes::insert_terminal_pane(&mut core, root, "capture", capture);
        core
    }

    /// Ticks the engine until `done`; a tick may fail for want of a supervisor.
    fn tick_until(core: &mut Core, what: &str, done: impl Fn(&Core) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(core) {
            assert!(Instant::now() < deadline, "{what}");
            core.poll_output().ok();
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Ticks until the captures that waited for the build are settled.
    fn finish_restore(core: &mut Core) {
        tick_until(core, "the restore never finished", |core| {
            core.capture_restore.is_empty()
        });
    }

    /// The reason a pane's capture stopped, which must be off.
    fn stop_reason(core: &Core) -> String {
        let capture = core.panes[PANE].stored.pane.experiment_capture.clone();
        let capture = capture.unwrap();
        assert!(!capture.on);
        capture.failure.unwrap()
    }

    fn signed_connection(core: &mut Core) -> String {
        let (sender, _receiver) = outbox::channel();
        let connection = core.connect(true, true, sender).unwrap();
        let hello = json!({
            "jsonrpc":"2.0","id":"hello","method":"hello",
            "params":{"client":"capture-test","clientVersion":"1","protocolVersion":1}
        });
        assert!(matches!(
            core.dispatch(&connection, hello).unwrap(),
            Outcome::Reply(_)
        ));
        connection
    }

    /// Takes the engine back from `shared`, once no lock is held on it, and removes its
    /// data directory.
    fn remove_shared(shared: Arc<Mutex<Core>>, directory: std::path::PathBuf) {
        let core = Arc::into_inner(shared).unwrap().into_inner().unwrap();
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn enabling_a_capture_while_the_build_is_hashing_waits_off_the_lock() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-build-{}", Uuid::new_v4()));
        let mut core = terminal_core(&directory, false, &CELL);
        let connection = signed_connection(&mut core);

        let enable = json!({
            "jsonrpc":"2.0","id":"enable","method":"pane.experiment.set",
            "params":{"paneId":PANE,"on":true,"idempotencyKey":"enable-once"}
        });
        let Outcome::Offload(offload) = core.dispatch(&connection, enable).unwrap() else {
            panic!("the request must wait for the build off the lock")
        };
        assert!(core.terminal_captures.is_empty());

        let shared = Arc::new(Mutex::new(core));
        let (_, work, _) = offload.into_parts();
        let waiting = thread::spawn(work);
        // The hash is still running: the lock is free, and the engine ticks through it.
        for _ in 0..5 {
            let mut core = shared.try_lock().unwrap();
            core.poll_output().ok();
            assert!(core.terminal_captures.is_empty());
            drop(core);
            thread::sleep(Duration::from_millis(5));
        }
        GATE.release();
        let apply = waiting.join().unwrap();
        let mut core = shared.lock().unwrap();
        let Outcome::Reply(reply) = apply(&mut core).unwrap() else {
            panic!("the request must answer once the build is known")
        };
        assert_eq!(reply["capture"]["on"], true);
        assert!(core.terminal_captures.contains_key(PANE));
        drop(core);
        remove_shared(shared, directory);
    }

    #[test]
    fn a_capture_that_was_on_at_open_is_restored_after_the_hash_without_delaying_open() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-restore-{}", Uuid::new_v4()));
        let mut core = terminal_core(&directory, true, &CELL);

        // The hash is still running. Restoring returns at once and leaves the pane waiting.
        core.restore_experiment_captures().unwrap();
        assert!(core.terminal_captures.is_empty());
        assert!(core.capture_restore.contains(PANE));
        core.apply_background();
        assert!(core.capture_restore.contains(PANE));

        GATE.release();
        tick_until(&mut core, "the capture was never restored", |core| {
            !core.terminal_captures.is_empty()
        });
        assert!(core.capture_restore.is_empty());
        assert_eq!(
            core.panes[PANE].stored.pane.experiment_capture,
            Some(PaneExperimentCapture {
                on: true,
                failure: None
            })
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_hook_that_comes_while_captures_wait_for_the_build_waits_off_the_lock_and_settles_them() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-hook-{}", Uuid::new_v4()));
        let mut core = terminal_core(&directory, true, &CELL);
        let connection = signed_connection(&mut core);
        core.restore_experiment_captures().unwrap();
        assert!(core.capture_restore.contains(PANE));

        let hook = json!({
            "jsonrpc":"2.0","id":"hook","method":"pane.experiment.hook",
            "params":{"event":"Stop","capability":"x".repeat(64),"idempotencyKey":"hook-once"}
        });
        let Outcome::Offload(offload) = core.dispatch(&connection, hook).unwrap() else {
            panic!("a hook must not be answered as a no-op while its capture is restoring")
        };
        let shared = Arc::new(Mutex::new(core));
        let (_, work, _) = offload.into_parts();
        let waiting = thread::spawn(work);
        GATE.release();
        let apply = waiting.join().unwrap();
        let mut core = shared.lock().unwrap();
        // The hook is not the engine's own channel, so it is refused; the point is that the
        // capture is running by the time the hook is judged.
        assert!(apply(&mut core).is_err());
        assert!(core.capture_restore.is_empty());
        assert!(core.terminal_captures.contains_key(PANE));
        drop(core);
        remove_shared(shared, directory);
    }

    #[test]
    fn a_restore_for_a_pane_that_was_turned_off_meanwhile_starts_nothing() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-off-{}", Uuid::new_v4()));
        let mut core = terminal_core(&directory, true, &CELL);
        core.restore_experiment_captures().unwrap();
        let pane = core.panes.get_mut(PANE).unwrap();
        pane.stored.pane.experiment_capture = Some(PaneExperimentCapture {
            on: false,
            failure: None,
        });
        GATE.release();
        finish_restore(&mut core);
        assert!(core.terminal_captures.is_empty());
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_failed_hash_stops_the_restored_capture_with_its_reason_and_is_not_retried() {
        static CELL: EngineBuildCell =
            EngineBuildCell::new(|| Err(errors::internal("executable unreadable")));
        let directory = std::env::temp_dir().join(format!("sn-capture-fail-{}", Uuid::new_v4()));
        let mut core = terminal_core(&directory, true, &CELL);
        core.restore_experiment_captures().unwrap();
        finish_restore(&mut core);
        assert!(stop_reason(&core).contains("executable unreadable"));
        // The failure is known now: turning the capture on again answers at once, with it.
        assert!(core.capture_build_ready());
        assert!(core.capture_build().is_err());
        crate::test_paths::remove_after(core, directory);
    }

    /// A supervisor that answers `Drain` with a fixed text at sequence 0 and records the
    /// sequence of every drain.
    struct Recorder {
        drains: Arc<Mutex<Vec<u64>>>,
        generation: u32,
    }

    const TEXT: &[u8] = b"text from before the restart\r\n";

    impl SupervisorPort for Recorder {
        fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
            let Request::Drain { from_seq, .. } = request else {
                return Ok(Response::Ok);
            };
            self.drains.lock().unwrap().push(from_seq);
            let bytes = if from_seq == 0 { TEXT } else { b"" };
            let next_seq = from_seq + bytes.len() as u64;
            Ok(Response::Output(Output {
                from_seq,
                next_seq,
                end_seq: next_seq,
                bytes: STANDARD.encode(bytes),
                gap: None,
                exited: false,
            }))
        }

        fn supervisor_id(&self) -> &str {
            "capture-supervisor"
        }

        fn supervisor_generation(&self) -> u32 {
            self.generation
        }
    }

    fn recording_core(root: &Path, cell: &'static EngineBuildCell) -> (Core, Arc<Mutex<Vec<u64>>>) {
        let mut core = terminal_core(root, true, cell);
        let drains = Arc::new(Mutex::new(Vec::new()));
        core.link_supervisor(Box::new(Recorder {
            drains: Arc::clone(&drains),
            generation: core.generation as u32,
        }));
        (core, drains)
    }

    #[test]
    fn a_pane_waiting_for_its_capture_is_not_drained_and_loses_no_byte_when_it_is_restored() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-drain-{}", Uuid::new_v4()));
        let (mut core, drains) = recording_core(&directory, &CELL);
        core.restore_experiment_captures().unwrap();
        for _ in 0..5 {
            core.poll_output().unwrap();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(drains.lock().unwrap().is_empty(), "drained while it waited");

        GATE.release();
        tick_until(&mut core, "the capture was never restored", |core| {
            !core.terminal_captures.is_empty()
        });
        core.poll_output().unwrap();
        assert_eq!(drains.lock().unwrap().first(), Some(&0));
        // Closing the capture lets its worker write what it was given.
        drop(core.terminal_captures.remove(PANE));
        let build = core.engine_build.ready().unwrap().unwrap();
        let rows = replay_rows(&core.observation_root().unwrap(), PANE, build);
        assert!(
            rows.iter()
                .any(|row| matches!(row, RecordedRow::Observation(frame)
                if frame.lines.iter().any(|line| line.contains("text from before the restart")))),
            "{rows:?}"
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_hash_that_never_finishes_stops_the_waiting_captures_and_the_panes_drain_again() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-stuck-{}", Uuid::new_v4()));
        let (mut core, drains) = recording_core(&directory, &CELL);
        core.restore_experiment_captures().unwrap();
        assert!(core.capture_restore_pending(PANE));
        core.poll_output().unwrap();
        assert!(drains.lock().unwrap().is_empty());

        // The wait is over, though the hash still runs.
        core.capture_restore_deadline = Some(Instant::now() - Duration::from_secs(1));
        core.poll_output().unwrap();
        assert!(!core.capture_restore_pending(PANE));
        assert!(stop_reason(&core).contains("not known within 60 s"));
        assert!(core.terminal_captures.is_empty());
        // The read starts with the tick and is made by a job; the pane drains again.
        tick_until(&mut core, "the pane never drained", |_| {
            !drains.lock().unwrap().is_empty()
        });
        assert_eq!(drains.lock().unwrap().first(), Some(&0));
        // The hash that ends later restores nothing.
        GATE.release();
        tick_until(&mut core, "the job never finished", |core| {
            !core.background.is_active(RESTORE_JOB)
        });
        assert!(core.terminal_captures.is_empty());
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_request_that_cannot_succeed_is_refused_before_it_waits_for_the_build() {
        static GATE: Gate = Gate::new();
        static CELL: EngineBuildCell = EngineBuildCell::new(|| GATE.hash());
        let directory = std::env::temp_dir().join(format!("sn-capture-bad-{}", Uuid::new_v4()));
        let mut core = terminal_core(&directory, false, &CELL);
        let connection = signed_connection(&mut core);
        let missing = json!({
            "jsonrpc":"2.0","id":"missing","method":"pane.experiment.set",
            "params":{"paneId":"no-such-pane","on":true,"idempotencyKey":"missing"}
        });
        let error = core.dispatch(&connection, missing).err().unwrap();
        assert_eq!(errors::kind(&error), soyeht_protocol::ErrorKind::NotFound);
        core.panes.get_mut(PANE).unwrap().stored.pane.mode = PaneMode::Editor;
        let editor = json!({
            "jsonrpc":"2.0","id":"editor","method":"pane.experiment.set",
            "params":{"paneId":PANE,"on":true,"idempotencyKey":"editor"}
        });
        assert!(core.dispatch(&connection, editor).is_err());
        GATE.release();
        crate::test_paths::remove_after(core, directory);
    }
}
