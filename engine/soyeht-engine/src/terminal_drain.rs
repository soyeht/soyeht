//! The terminal path of the tick: new output of a shell pane moves from the supervisor into the
//! pane's screen, and the supervisor is told it may drop what is safe on disk.
//!
//! The tick holds the Core lock, so it never waits for the supervisor or the disk (ADR 0079,
//! ADR 0095, ADR 0117). Each pane has one background job: it asks the supervisor for the new
//! output with no lock, and the output is put into the screen under the lock when the job is
//! applied. It writes the output to the pane's segment file and goes on; the flush runs on a
//! background worker and the acknowledgement follows it, also off the lock. The supervisor
//! keeps the output until then, so a crash before the flush loses nothing: the engine drains it
//! again from its last durable position, and so it does for a read that no longer fits the
//! pane when it is applied.

use crate::supervisor_watch::ReadSummary;
use crate::{
    core::Core,
    errors::{self, Result},
    supervisor_handle::Supervisor,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_platform::log;
use soyeht_protocol::PaneState;
use soyeht_ptyd::wire::{Output, Request, Response};
use soyeht_store::segments::Record;
use std::sync::Arc;

/// The background job that makes a pane's output durable and then acknowledges it.
fn flush_job(pane_id: &str) -> String {
    format!("pane.flush:{pane_id}")
}

/// The background job that reads a pane's new output from the supervisor.
fn drain_job(pane_id: &str) -> String {
    format!("pane.drain:{pane_id}")
}

/// What a drain job read from the supervisor, with no lock.
pub(crate) struct Read {
    pub(crate) output: Output,
    /// The exit status the supervisor recorded, looked up when the output says the process
    /// exited. The supervisor records it just after it marks the process exited, so a status
    /// that is not there yet stays unknown rather than guessed.
    pub(crate) exit_code: Option<i64>,
    /// The output says the process exited and the pane did not know yet. A process that is
    /// already known to be dead keeps saying so until it is closed; that is not news.
    exit_news: bool,
}

/// Where a drain job started, to tell when it is applied whether the pane is still the one it
/// read for.
struct Started {
    instance_id: String,
    sequence_base: u64,
    next: u64,
    supervisor: Supervisor,
}

/// Whether a drain found something the tick should apply at once: output, a gap, an exit the
/// pane did not know, or an error (a lost link is dropped when its error is applied).
fn has_news(read: &Result<Read>) -> bool {
    !read.as_ref().is_ok_and(|read| {
        read.output.bytes.is_empty() && !read.exit_news && read.output.gap.is_none()
    })
}

pub(crate) fn read_output(
    supervisor: &Supervisor,
    instance_id: &str,
    from_seq: u64,
    look_up_exit: bool,
) -> Result<Read> {
    let Response::Output(output) = supervisor.call(Request::Drain {
        instance_id: instance_id.to_owned(),
        from_seq,
    })?
    else {
        return Err(errors::ptyd_unavailable("Invalid output response"));
    };
    let exit_news = output.exited && look_up_exit;
    let exit_code = if exit_news {
        exit_code_of(supervisor, instance_id)?
    } else {
        None
    };
    Ok(Read {
        output,
        exit_code,
        exit_news,
    })
}

/// The exit status the supervisor recorded for an instance, if it has recorded one.
fn exit_code_of(supervisor: &Supervisor, instance_id: &str) -> Result<Option<i64>> {
    let Response::Instances(instances) = supervisor.call(Request::List)? else {
        return Err(errors::ptyd_unavailable("Invalid process registry"));
    };
    Ok(instances
        .iter()
        .find(|i| i.instance_id == instance_id)
        .and_then(|i| i.exit_code)
        .map(i64::from))
}

impl Core {
    /// Starts reading what the supervisor holds for the shell pane `id`, unless the last read
    /// of the pane has not been applied yet. The read is applied by
    /// [`Core::finish_terminal_drain`].
    pub(crate) fn drain_terminal(&mut self, id: &str) -> Result<()> {
        let job = drain_job(id);
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let saved = self.panes[id].stored.clone();
        let instance_id = errors::invariant(
            saved.instance_id.clone(),
            "Running pane has no adopted process",
        )?;
        // With the supervisor's notices a process is read when it has output, not on every tick.
        if self.watch.is_active() && !self.watch.should_read(&instance_id, self.polling_woken()) {
            return Ok(());
        }
        let next = self.panes[id].screen.next_offset();
        let seq = next.saturating_sub(saved.sequence_base);
        let supervisor = self.supervisor()?;
        let started = Started {
            instance_id: instance_id.clone(),
            sequence_base: saved.sequence_base,
            next,
            supervisor: supervisor.clone(),
        };
        let look_up_exit = saved.pane.state != PaneState::Dead;
        let pane_id = id.to_owned();
        self.spawn_prompt_background(
            job,
            move || read_output(&supervisor, &instance_id, seq, look_up_exit),
            move |core, read| core.finish_terminal_drain(&pane_id, &started, read),
            has_news,
        )
    }

    /// Puts what a drain job read into the screen of pane `id`. A read that no longer fits the
    /// pane (its process was replaced, it closed, or its screen moved) is dropped: the
    /// supervisor still holds the output.
    fn finish_terminal_drain(
        &mut self,
        id: &str,
        started: &Started,
        read: Result<Read>,
    ) -> Result<()> {
        // A failed read on a link that was replaced meanwhile says nothing about the new one.
        let Some(Read {
            output, exit_code, ..
        }) = self.settle_if_current(&started.supervisor, read)?
        else {
            return Ok(());
        };
        let summary = ReadSummary {
            next_seq: output.next_seq,
            end_seq: output.end_seq,
            exited: output.exited,
            found: !output.bytes.is_empty() || output.gap.is_some(),
        };
        let Some(running) = self.panes.get(id) else {
            return Ok(());
        };
        let saved = running.stored.clone();
        if saved.closed
            || saved.instance_id.as_deref() != Some(started.instance_id.as_str())
            || saved.sequence_base != started.sequence_base
            || running.screen.next_offset() != started.next
            || self.agents.running.contains_key(id)
            || self.capture_restore_pending(id)
        {
            return Ok(());
        }
        let running = errors::invariant(
            self.panes.get_mut(id),
            "Pane disappeared while polling output",
        )?;
        let from = saved
            .sequence_base
            .checked_add(output.from_seq)
            .ok_or_else(|| errors::internal("Output offset exhausted"))?;
        let mut taken = false;
        if from > running.screen.next_offset() {
            running.screen.append_deferred(Record::Gap {
                from: running.screen.next_offset(),
                to: from,
            })?;
            taken = true;
        }
        let bytes = STANDARD
            .decode(output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid output encoding"))?;
        let capture_bytes = if !bytes.is_empty() && self.terminal_captures.contains_key(id) {
            let shared = Arc::new(bytes);
            running
                .screen
                .append_shared_data_deferred(from, Arc::clone(&shared))?;
            taken = true;
            Some(shared)
        } else if !bytes.is_empty() {
            running.screen.append_deferred(Record::Data {
                offset: from,
                bytes,
            })?;
            taken = true;
            None
        } else {
            None
        };
        if taken {
            running.screen.take_in_through(output.next_seq);
        }
        self.watch.note_read(&started.instance_id, &summary);
        self.acknowledge_when_durable(id, &started.instance_id)?;
        if let Some(bytes) = capture_bytes {
            self.observe_terminal(id, bytes)?;
        }
        if output.exited && saved.pane.state != PaneState::Dead {
            let mut dead = saved;
            dead.pane.exit_code = exit_code;
            dead.pane.state = PaneState::Dead;
            dead.pane.phrase = "Shell exited".into();
            self.save_runtime(id, dead)?;
        }
        Ok(())
    }

    /// Tells the supervisor it may drop the output of pane `id` once that output is on the
    /// disk. The flush and the acknowledgement run off the lock, one job at a time per pane;
    /// output that arrives while it runs is flushed by the next one.
    fn acknowledge_when_durable(&mut self, id: &str, instance_id: &str) -> Result<()> {
        let job = flush_job(id);
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let Some(durable) = self
            .panes
            .get_mut(id)
            .and_then(|running| running.screen.take_durable())
        else {
            return Ok(());
        };
        let supervisor = match self.supervisor() {
            Ok(supervisor) => supervisor,
            Err(error) => {
                if let Some(running) = self.panes.get_mut(id) {
                    running.screen.restore_durable(&durable);
                }
                return Err(error);
            }
        };
        let request = Request::Ack {
            instance_id: instance_id.to_owned(),
            seq: durable.through,
        };
        let pane_id = id.to_owned();
        let durable = Arc::new(durable);
        let (waiting, applying) = (Arc::clone(&durable), Arc::clone(&durable));
        let used = supervisor.clone();
        let started = self.spawn_background(
            job,
            move || {
                // With nothing written to the disk (recording is off) there is nothing to wait
                // for.
                if waiting.pending() {
                    waiting.flush()?;
                }
                Ok(supervisor.call(request))
            },
            move |core, flushed| match flushed {
                Ok(acknowledged) => {
                    let acknowledged = core.settle_if_current(&used, acknowledged);
                    if !matches!(acknowledged, Ok(Some(_))) {
                        // The flush is done: only the acknowledgement is owed again.
                        core.owe_acknowledgement(&pane_id, applying.through);
                    }
                    acknowledged.map(drop)
                }
                Err(error) => {
                    // The next flush retries what this one could not do.
                    if let Some(running) = core.panes.get_mut(&pane_id) {
                        running.screen.restore_durable(&applying);
                    }
                    Err(error)
                }
            },
        );
        if let Err(error) = started {
            // No flush runs: the next tick asks again. The other panes are not held up by it.
            if let Some(running) = self.panes.get_mut(id) {
                running.screen.restore_durable(&durable);
            }
            log::failure("engine.terminal_flush_not_started", &error);
        }
        Ok(())
    }

    fn owe_acknowledgement(&mut self, id: &str, through: u64) {
        if let Some(running) = self.panes.get_mut(id) {
            running.screen.take_in_through(through);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ports::SupervisorPort, test_panes::insert_terminal_pane};
    use soyeht_ptyd::wire::Output;
    use std::{
        collections::VecDeque,
        fs,
        sync::Mutex,
        time::{Duration, Instant},
    };
    use uuid::Uuid;

    /// A supervisor holding some output, and a record of what it was told to drop.
    struct Holding {
        output: VecDeque<&'static str>,
        position: u64,
        acknowledged: Arc<Mutex<Vec<u64>>>,
        /// How many acknowledgements are refused before one is taken.
        refuse: Arc<Mutex<u32>>,
        /// How many reads it was asked for.
        drains: Arc<std::sync::atomic::AtomicU32>,
        /// How much output it holds in all, so a read that returns one chunk is short of it.
        total: u64,
    }

    impl SupervisorPort for Holding {
        fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
            Ok(match request {
                Request::Drain { from_seq, .. } => {
                    self.drains
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let text = self.output.pop_front().unwrap_or("");
                    let next_seq = self.position + text.len() as u64;
                    self.position = next_seq;
                    Response::Output(Output {
                        from_seq,
                        next_seq,
                        end_seq: self.total.max(next_seq),
                        bytes: STANDARD.encode(text),
                        gap: None,
                        exited: false,
                    })
                }
                Request::Ack { seq, .. } => {
                    let mut refuse = self.refuse.lock().unwrap();
                    if *refuse > 0 {
                        *refuse -= 1;
                        return Err(soyeht_ptyd::Error::Remote {
                            kind: "refused".into(),
                            message: "refused".into(),
                        });
                    }
                    self.acknowledged.lock().unwrap().push(seq);
                    Response::Ok
                }
                _ => Response::Ok,
            })
        }

        fn supervisor_id(&self) -> &str {
            "holding"
        }

        fn supervisor_generation(&self) -> u32 {
            1
        }
    }

    type Fixture = (Core, Arc<Mutex<Vec<u64>>>, std::path::PathBuf, String);

    fn core_with_output(recording: bool) -> Fixture {
        core_refusing(recording, 0)
    }

    fn core_refusing(recording: bool, refusals: u32) -> Fixture {
        let root = std::env::temp_dir().join(format!("soyeht-drain-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut core = Core::open(&root).unwrap();
        let id = insert_terminal_pane(&mut core, &root, "shell", false);
        core.panes.get_mut(&id).unwrap().screen.recording = recording;
        let acknowledged = Arc::new(Mutex::new(Vec::new()));
        core.link_supervisor(Box::new(Holding {
            output: VecDeque::from(["hello", " world"]),
            position: 0,
            acknowledged: Arc::clone(&acknowledged),
            refuse: Arc::new(Mutex::new(refusals)),
            drains: Arc::default(),
            total: 11,
        }));
        (core, acknowledged, root, id)
    }

    /// Waits for `count` acknowledgements, and then for the flush job of pane `id` to be
    /// applied: the supervisor sees the acknowledgement before the job that sent it ends, and
    /// only an ended job lets the next flush start.
    fn wait_for_acknowledgements(core: &mut Core, id: &str, seen: &Mutex<Vec<u64>>, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while seen.lock().unwrap().len() < count || core.background.is_active(&flush_job(id)) {
            assert!(
                Instant::now() < deadline,
                "the output was never acknowledged"
            );
            core.apply_background();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Starts a read of pane `id` and applies it, as the next ticks do.
    fn drain_and_apply(core: &mut Core, id: &str) {
        core.drain_terminal(id).unwrap();
        apply_read(core, id);
    }

    /// Applies finished jobs until the read of pane `id` has been applied.
    fn apply_read(core: &mut Core, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while core.background.is_active(&drain_job(id)) {
            assert!(Instant::now() < deadline, "the read was never applied");
            core.apply_background();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn output_is_acknowledged_only_after_its_flush() {
        let (mut core, acknowledged, root, id) = core_with_output(true);

        // The supervisor records an acknowledgement under this lock, so holding it keeps the
        // flush job from finishing: the flush runs on its own thread and, left alone, could
        // acknowledge before the test looks.
        let held = acknowledged.lock().unwrap();
        drain_and_apply(&mut core, &id);

        // The tick wrote the output and went on: the supervisor still holds it.
        assert!(held.is_empty());
        assert_eq!(core.panes[&id].screen.next_offset(), 5);
        drop(held);
        wait_for_acknowledgements(&mut core, &id, &acknowledged, 1);
        assert_eq!(*acknowledged.lock().unwrap(), vec![5]);

        // Output that arrives later is flushed and acknowledged by the next job.
        drain_and_apply(&mut core, &id);
        wait_for_acknowledgements(&mut core, &id, &acknowledged, 2);
        assert_eq!(*acknowledged.lock().unwrap(), vec![5, 11]);
        crate::test_paths::remove_after(core, root);
    }

    /// A core whose supervisor holds no output and counts the reads it is asked for.
    fn core_counting() -> (
        Core,
        std::path::PathBuf,
        String,
        Arc<std::sync::atomic::AtomicU32>,
    ) {
        let (mut core, _, root, id) = core_with_output(false);
        let drains = Arc::new(std::sync::atomic::AtomicU32::new(0));
        core.link_supervisor(Box::new(Holding {
            output: VecDeque::new(),
            position: 0,
            acknowledged: Arc::default(),
            refuse: Arc::default(),
            drains: Arc::clone(&drains),
            total: 0,
        }));
        (core, root, id, drains)
    }

    /// Runs `ticks` ticks of the terminal path, applying each read that starts.
    fn tick_the_terminal_path(core: &mut Core, id: &str, ticks: usize) {
        for _ in 0..ticks {
            core.drain_panes().unwrap();
            apply_read(core, id);
        }
    }

    #[test]
    fn with_the_supervisors_notices_an_idle_pane_makes_no_request_and_a_marked_one_makes_one() {
        let (mut core, root, id, drains) = core_counting();
        let reads = || drains.load(std::sync::atomic::Ordering::SeqCst);

        // Polling, as it always was: one request per tick.
        tick_the_terminal_path(&mut core, &id, 20);
        assert_eq!(reads(), 20);

        // Listening: a second of ticks asks nothing (the sweep of the second is spent first).
        core.watch.pretend_active();
        core.watch.sweep(std::iter::empty());
        let before = reads();
        tick_the_terminal_path(&mut core, &id, 100);
        assert_eq!(reads(), before, "an idle pane is not read");

        // A notice marks it: it is read once, and then left alone again.
        core.watch.pretend_notice("shell-instance", 0);
        tick_the_terminal_path(&mut core, &id, 100);
        assert_eq!(reads(), before + 1);
        assert!(!core.pane_marked(&id));
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn a_read_that_stops_short_is_repeated_by_the_next_woken_tick_until_the_burst_is_read() {
        let (mut core, acknowledged, root, id) = core_with_output(false);
        core.watch.pretend_active();
        core.watch.sweep(std::iter::empty());
        // The notice of the first chunk said 5; the supervisor holds 11 in all, and a read returns
        // one chunk at a time.
        core.watch.pretend_notice("shell-instance", 5);

        core.poll_woken().unwrap();
        apply_read(&mut core, &id);
        assert_eq!(core.panes[&id].screen.next_offset(), 5);
        assert!(
            core.pane_marked(&id),
            "the rest of the burst is still to read"
        );

        core.poll_woken().unwrap();
        apply_read(&mut core, &id);
        assert_eq!(core.panes[&id].screen.next_offset(), 11);
        assert!(!core.pane_marked(&id));
        wait_for_acknowledgements(&mut core, &id, &acknowledged, 1);
        assert_eq!(core.watch.missed(), 0);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn the_sweep_reads_every_pane_once_a_second_and_a_quiet_one_is_no_miss() {
        let (mut core, root, id, drains) = core_counting();
        core.watch.pretend_active();

        tick_the_terminal_path(&mut core, &id, 50);

        // The first tick sweeps; the rest of the second reads nothing.
        assert_eq!(drains.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(core.watch.missed(), 0);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn closing_a_pane_releases_the_output_its_screen_held() {
        let (mut core, acknowledged, root, id) = core_with_output(true);
        drain_and_apply(&mut core, &id);
        wait_for_acknowledgements(&mut core, &id, &acknowledged, 1);
        assert_eq!(core.panes[&id].screen.held_bytes(), 5);

        core.panes.get_mut(&id).unwrap().stored.closed = true;
        core.finish_close(&id).unwrap();

        let screen = &core.panes[&id].screen;
        assert_eq!(screen.held_bytes(), 0);
        assert_eq!(screen.first_offset(), screen.next_offset());
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn output_nobody_records_is_acknowledged_at_once() {
        let (mut core, acknowledged, root, id) = core_with_output(false);

        drain_and_apply(&mut core, &id);

        // Nothing waits for the disk: the job only asks the supervisor.
        wait_for_acknowledgements(&mut core, &id, &acknowledged, 1);
        assert_eq!(*acknowledged.lock().unwrap(), vec![5]);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn a_quiet_pane_is_not_acknowledged_every_tick() {
        let (mut core, acknowledged, root, id) = core_with_output(true);
        let deadline = Instant::now() + Duration::from_secs(30);
        while acknowledged.lock().unwrap().last() != Some(&11) {
            assert!(
                Instant::now() < deadline,
                "the output was never acknowledged"
            );
            core.drain_terminal(&id).unwrap();
            core.apply_background();
            std::thread::sleep(Duration::from_millis(2));
        }
        let before = acknowledged.lock().unwrap().len();

        for _ in 0..10 {
            core.drain_terminal(&id).unwrap();
            core.apply_background();
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(acknowledged.lock().unwrap().len(), before);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn a_refused_acknowledgement_is_owed_again_after_the_flush_succeeded() {
        let (mut core, acknowledged, root, id) = core_refusing(true, 1);
        core.drain_terminal(&id).unwrap();

        // The first acknowledgement is refused; the position is kept and asked again, once the
        // job has cooled down, without writing the output a second time.
        let deadline = Instant::now() + Duration::from_secs(30);
        while acknowledged.lock().unwrap().is_empty() {
            assert!(
                Instant::now() < deadline,
                "the output was never acknowledged"
            );
            core.apply_background();
            core.drain_terminal(&id).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }

        // The retry covers the output drained meanwhile too.
        assert!(acknowledged.lock().unwrap()[0] >= 5);
        assert_eq!(core.panes[&id].screen.next_offset(), 11);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn a_read_for_a_process_that_was_replaced_meanwhile_is_dropped() {
        let (mut core, acknowledged, root, id) = core_with_output(true);
        core.drain_terminal(&id).unwrap();
        // The pane's process is replaced while the supervisor answers.
        core.panes.get_mut(&id).unwrap().stored.instance_id = Some("replacement".into());
        apply_read(&mut core, &id);

        // Nothing of the old process reached the screen, and nothing was acknowledged: the
        // supervisor still holds the output.
        assert_eq!(core.panes[&id].screen.next_offset(), 0);
        assert!(acknowledged.lock().unwrap().is_empty());
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn a_drain_that_finds_the_link_gone_drops_it() {
        let (mut core, _, root, id) = core_with_output(true);
        core.link_supervisor(Box::new(crate::ports::testing::FakeSupervisor::default()));

        // The fake has no answer for a drain: its transport failed.
        drain_and_apply(&mut core, &id);

        assert!(core.ptyd.is_none());
        crate::test_paths::remove_after(core, root);
    }

    /// The answers of a supervisor whose process exited: every drain says so, as it does until
    /// the process is closed, and the registry lists nothing.
    fn exited() -> Supervisor {
        let output = |from_seq| {
            Ok(Response::Output(Output {
                from_seq,
                next_seq: from_seq,
                end_seq: from_seq,
                bytes: String::new(),
                gap: None,
                exited: true,
            }))
        };
        Supervisor::new(Box::new(crate::ports::testing::FakeSupervisor {
            responses: VecDeque::from([output(0), Ok(Response::Instances(Vec::new()))]),
            ..Default::default()
        }))
    }

    #[test]
    fn a_dead_pane_that_is_still_open_does_not_wake_the_tick() {
        // The pane already knows its process is dead: the read says it again, which is no news.
        let known = read_output(&exited(), "instance", 0, false).unwrap();
        assert!(known.output.exited);
        assert!(!has_news(&Ok(known)));

        // The first time it is news, and the exit status is looked up.
        let fresh = read_output(&exited(), "instance", 0, true).unwrap();
        assert!(has_news(&Ok(fresh)));
    }
}
