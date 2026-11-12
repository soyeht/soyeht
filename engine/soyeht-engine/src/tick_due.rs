//! When the tick has to run next (ADR 0147, step 1b).
//!
//! The tick used to run every 10 ms and ask every subsystem whether it had work. It now sleeps
//! until the earliest of what the subsystems say, and is raised by an event. A subsystem says one
//! of three things:
//!
//! - **a deadline**: a retry, an autosave, a probe, a timeout, the end of a burst of file events,
//!   the next sweep of the supervisor's output. Its `due` function reports it to [`Next`];
//! - **now**: it has work that goes on without an event (an agent that is not at rest, a replay
//!   that has not caught up, a pane that has output no one has read), so the tick runs again
//!   after the floor of one tick interval;
//! - **nothing**: its work can only start from an event. An event raises the tick by one of
//!   three signals: a background job that ended and every hold of the Core lock that is not the
//!   tick's own (the kick of `Background`), a notice of the supervisor (the wake of `Background`),
//!   or a file event (the kick, raised from the event stream).
//!
//! The ceiling of one second is a safety net, not a schedule: a subsystem that forgot its
//! deadline is late by at most that, never silent. `Hooks::tick_ceiling` lets a test raise it to
//! show that no subsystem relies on it.

use crate::core::Core;
use std::time::{Duration, Instant};

/// The shortest time between the starts of two runs of the tick, which is also its period while
/// anything has work. Terminal output is read when the supervisor says so, not when the tick runs;
/// the floor only batches what arrives in small chunks, and paces the other subsystems.
pub(crate) const FLOOR: Duration = Duration::from_millis(10);
/// The longest the tick sleeps: the safety net behind the deadlines.
pub(crate) const CEILING: Duration = Duration::from_secs(1);

/// The soonest of what the subsystems asked for.
#[derive(Default)]
pub(crate) struct Next {
    soonest: Option<Duration>,
}

impl Next {
    /// The tick must run again within `wait`.
    pub(crate) fn within(&mut self, wait: Duration) {
        self.soonest = Some(self.soonest.map_or(wait, |soonest| soonest.min(wait)));
    }

    /// The tick must run again, after the floor.
    pub(crate) fn now(&mut self) {
        self.within(Duration::ZERO);
    }

    /// The tick must run when the engine's clock reaches `due_ms`.
    pub(crate) fn at_ms(&mut self, now_ms: i64, due_ms: i64) {
        let wait = u64::try_from(due_ms.saturating_sub(now_ms)).unwrap_or(0);
        self.within(Duration::from_millis(wait));
    }

    /// The tick must run at `deadline`.
    pub(crate) fn at(&mut self, deadline: Instant) {
        self.within(deadline.saturating_duration_since(Instant::now()));
    }

    pub(crate) fn soonest(&self) -> Option<Duration> {
        self.soonest
    }
}

/// What the tick remembers between runs.
#[derive(Default)]
pub(crate) struct State {
    /// A subscriber was left behind by the last pump: its outbox was full, or what it still has
    /// to be sent is more than one pump carries.
    pub(crate) streams_behind: bool,
}

/// How long the tick may sleep, from the end of a run: the earliest deadline of every subsystem
/// the tick drives, or the ceiling. Zero means it has work now.
pub(crate) fn wait(core: &Core) -> Duration {
    let ceiling = core.hooks.tick_ceiling().unwrap_or(CEILING);
    let now = core.now_ms();
    let mut next = Next::default();
    file_and_service_dues(core, &mut next, now);
    core.link_due(&mut next, now);
    // The pump runs whether or not there is a link.
    if core.streams_behind() {
        next.now();
    }
    // The rest of the tick runs only with a link to the supervisor, and the connection that
    // makes one kicks the tick.
    if core.ptyd.is_some() {
        supervisor_dues(core, &mut next, now);
        task_dues(core, &mut next, now);
    }
    next.soonest().map_or(ceiling, |wait| wait.min(ceiling))
}

/// The editor, the project panel, the skills, the local servers, the input receipts and the web
/// commands: what the tick does before it needs the supervisor.
fn file_and_service_dues(core: &Core, next: &mut Next, now: i64) {
    core.editor_due(next, now);
    if let Some(due) = core.project_watches.next_due_ms() {
        next.at_ms(now, due);
    }
    if let Some(due) = core.skill_watches.next_due_ms() {
        next.at_ms(now, due);
    }
    if core.web.unavailable().is_none() {
        next.at_ms(now, core.web.next_refresh_ms());
    }
    core.input_receipts_due(next, now);
    core.web_commands_due(next, now);
}

/// What the tick does for the panes and the processes behind them.
fn supervisor_dues(core: &Core, next: &mut Next, now: i64) {
    core.retries_due(next, now);
    core.output_due(next, now);
    crate::languages::watching::due(core, next);
    if !core.harness_switches.is_empty()
        || !core.terminal_captures.is_empty()
        || !core.agent_captures.is_empty()
    {
        next.now();
    }
    if let Some(deadline) = core.capture_restore_deadline {
        next.at(deadline);
    }
    core.retired_processes_due(next, now);
}

/// The tasks. A task has work when the store says its poll is not at rest, or when a poll of its
/// own is due; a write to the store, by a request or by the tick itself, makes the tick run again
/// (see [`crate::tick`]).
fn task_dues(core: &Core, next: &mut Next, now: i64) {
    if let Some(wait) = core.store.next_poll_wait(now) {
        next.within(wait);
    }
    if let Some(due) = core.task_revision_due.values().min() {
        next.at_ms(now, *due);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        hooks::Hooks,
        input_receipts::{LINGER_MS, Waiting},
        offload::Background,
        ports::testing::EmptyPortScan,
        supervisor_link::tests::Manual,
        supervisor_watch::ReadSummary,
        test_panes::{Answering, insert_terminal_pane, quiet_read},
    };
    use soyeht_ptyd::wire::{Request, Response};
    use std::{
        path::PathBuf,
        sync::{Arc, atomic::AtomicI64},
    };

    /// The hooks of a quiet core: the skill folder is read on the probe, not by file events, whose
    /// start on a thread of its own would make the first runs of the tick depend on the machine.
    struct Quiet(Option<Duration>);

    impl Hooks for Quiet {
        fn poll_skills_only(&self) -> bool {
            true
        }

        fn tick_ceiling(&self) -> Option<Duration> {
            self.0
        }
    }

    /// A core with a supervisor that has nothing to say, whose subsystems have all run and whose
    /// first bursts of file events have settled. Its clock is the test's.
    pub(crate) fn quiet_core_with(ceiling: Option<Duration>) -> (Core, PathBuf) {
        let directory = crate::supervisor_link::tests::short_directory();
        let mut core = Core::open_with(&directory, Arc::new(Quiet(ceiling))).unwrap();
        let clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
        core.clock = clock.clone();
        core.web = crate::web::Service::new(Box::new(EmptyPortScan));
        core.background = Background::inline();
        core.link_supervisor(Box::new(Answering(|request| {
            Ok(match request {
                Request::Drain { from_seq, .. } => quiet_read(from_seq),
                _ => Response::Ok,
            })
        })));
        for _ in 0..3 {
            core.poll_output().unwrap();
            clock.0.fetch_add(100, std::sync::atomic::Ordering::SeqCst);
        }
        (core, directory)
    }

    pub(crate) fn quiet_core() -> (Core, PathBuf) {
        quiet_core_with(None)
    }

    #[test]
    fn the_soonest_of_several_deadlines_wins() {
        let mut next = Next::default();
        assert_eq!(next.soonest(), None);
        next.at_ms(1_000, 1_500);
        next.at_ms(1_000, 1_200);
        next.within(Duration::from_secs(3));
        assert_eq!(next.soonest(), Some(Duration::from_millis(200)));
    }

    #[test]
    fn a_deadline_in_the_past_is_now() {
        let mut next = Next::default();
        next.at_ms(5_000, 100);
        assert_eq!(next.soonest(), Some(Duration::ZERO));
        let mut next = Next::default();
        next.at(Instant::now() - Duration::from_secs(1));
        assert_eq!(next.soonest(), Some(Duration::ZERO));
    }

    #[test]
    fn an_engine_with_nothing_to_do_sleeps_until_the_ceiling() {
        let (core, directory) = quiet_core();
        assert_eq!(wait(&core), CEILING);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn the_ceiling_is_the_hooks_and_a_deadline_still_comes_first() {
        let (core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        let waited = wait(&core);
        assert!(waited > CEILING, "the ceiling moved: {waited:?}");
        assert!(
            waited <= Duration::from_secs(5),
            "the scan of the local servers, due in five seconds, comes first: {waited:?}"
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn the_probe_of_the_skill_folders_is_the_first_deadline_of_a_quiet_engine() {
        let (core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        // The probe was due two seconds after the first run, three hundred milliseconds ago.
        assert_eq!(wait(&core), Duration::from_millis(1_700));
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_deadline_that_has_passed_is_work_for_the_next_run() {
        let (mut core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        let moved = core.now_ms() + 2_000;
        core.clock = Arc::new(Manual(AtomicI64::new(moved)));
        assert_eq!(wait(&core), Duration::ZERO);
        core.poll_output().unwrap();
        assert!(wait(&core) > Duration::ZERO, "the run took the probe");
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn an_outcome_waiting_for_the_next_input_is_a_deadline() {
        // Queued so long ago that the linger ends before the probe of the skill folders (1.7 s)
        // and the ceiling: the outcome's deadline is what the wait is.
        let (mut core, directory) = quiet_core();
        let left = 500;
        let queued = core.now_ms() - LINGER_MS + left;
        core.input_receipts
            .push(Waiting::for_test("a", "op-a", queued));
        assert_eq!(wait(&core), Duration::from_millis(left.unsigned_abs()));
        crate::test_paths::remove_after(core, directory);
    }

    /// The wait of a quiet engine whose ceiling is a minute, which is the probe of the skill
    /// folders: any deadline the tests add is sooner.
    const PROBE_WAIT: Duration = Duration::from_millis(1_700);

    #[test]
    fn a_subscriber_left_behind_by_the_pump_is_work_for_the_next_run() {
        let (mut core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        core.note_streams_behind(true);
        assert_eq!(wait(&core), Duration::ZERO);
        core.note_streams_behind(false);
        assert_eq!(wait(&core), PROBE_WAIT);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn the_revision_of_a_task_is_a_deadline() {
        let (mut core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        let due = core.now_ms() + 300;
        core.task_revision_due.insert(("w".into(), "t".into()), due);
        core.task_revision_due
            .insert(("w".into(), "later".into()), due + 5_000);
        assert_eq!(wait(&core), Duration::from_millis(300));
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn the_wait_for_a_capture_to_be_restored_is_a_deadline() {
        let (mut core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        core.capture_restore_deadline = Some(Instant::now() + Duration::from_millis(300));
        let waited = wait(&core);
        assert!(waited <= Duration::from_millis(300), "{waited:?}");
        core.capture_restore_deadline = None;
        assert_eq!(wait(&core), PROBE_WAIT);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_task_poll_that_is_not_at_rest_is_work_and_one_that_rests_is_a_deadline() {
        let (core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        let now = core.now_ms();
        // A poll that began and has not rested yet, as one that found a row to wait on.
        let seen = core.store.begin_poll("waiting_row", now).unwrap();
        assert_eq!(wait(&core), Duration::ZERO);
        core.store.rest("waiting_row", seen, now + 300);
        assert_eq!(wait(&core), Duration::from_millis(300));
        core.store.rest("waiting_row", seen, i64::MAX);
        assert_eq!(wait(&core), PROBE_WAIT, "only a write wakes it");
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_shell_is_polled_until_the_watch_is_heard_and_then_read_on_a_notice_or_the_sweep() {
        let (mut core, directory) = quiet_core_with(Some(Duration::from_secs(60)));
        let id = insert_terminal_pane(&mut core, &directory, "shell", false);
        assert_eq!(wait(&core), Duration::ZERO, "polling, as it always was");

        // Listening: the first sweep marks the pane, and a read that reaches the end clears it.
        core.watch.pretend_active();
        core.watch.sweep(std::iter::once("shell-instance"));
        assert!(core.pane_marked(&id));
        assert_eq!(wait(&core), Duration::ZERO, "a marked pane is read");
        core.watch.note_read(
            "shell-instance",
            &ReadSummary {
                next_seq: 0,
                end_seq: 0,
                exited: false,
                found: false,
            },
        );
        let waited = wait(&core);
        assert!(
            waited > Duration::from_millis(900) && waited <= crate::supervisor_watch::SWEEP,
            "an idle pane waits for the next sweep: {waited:?}"
        );

        core.watch.pretend_notice("shell-instance", 5);
        assert_eq!(wait(&core), Duration::ZERO, "a notice marks it");
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_link_that_is_down_is_connected_again_when_its_delay_ends() {
        let directory = crate::supervisor_link::tests::short_directory();
        let mut core = Core::open(&directory).unwrap();
        core.clock = Arc::new(Manual(AtomicI64::new(1_000_000)));
        core.web = crate::web::Service::new(Box::new(EmptyPortScan));
        core.background = Background::inline();
        // No supervisor listens: the connection failed and the retry waits.
        core.poll_output().unwrap();
        assert!(core.ptyd.is_none());
        let waited = wait(&core);
        assert!(
            waited > Duration::ZERO && waited <= Duration::from_secs(2),
            "the retry delay of a link that is down: {waited:?}"
        );
        crate::test_paths::remove_after(core, directory);
    }
}
