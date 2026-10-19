//! The terminal path of the tick: new output of a shell pane moves from the supervisor into the
//! pane's screen, and the supervisor is told it may drop what is safe on disk.
//!
//! The tick holds the Core lock, so it never waits for the disk (ADR 0079, ADR 0095). It writes
//! the output to the pane's segment file and goes on; the flush runs on a background worker and
//! the acknowledgement follows it. The supervisor keeps the output until then, so a crash
//! before the flush loses nothing: the engine drains it again from its last durable position.

use crate::{
    core::Core,
    errors::{self, Result},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_platform::log;
use soyeht_protocol::PaneState;
use soyeht_ptyd::wire::{Request, Response};
use soyeht_store::segments::Record;
use std::sync::Arc;

/// The background job that makes a pane's output durable and then acknowledges it.
fn flush_job(pane_id: &str) -> String {
    format!("pane.flush:{pane_id}")
}

impl Core {
    /// Takes what the supervisor holds for the shell pane `id` into its screen.
    pub(crate) fn drain_terminal(&mut self, id: &str) -> Result<()> {
        let saved = self.panes[id].stored.clone();
        let instance_id = errors::invariant(
            saved.instance_id.clone(),
            "Running pane has no adopted process",
        )?;
        let next = self.panes[id].screen.next_offset();
        let seq = next.saturating_sub(saved.sequence_base);
        let Response::Output(output) = self.pty_request(Request::Drain {
            instance_id: instance_id.clone(),
            from_seq: seq,
        })?
        else {
            return Err(errors::ptyd_unavailable("Invalid output response"));
        };
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
        self.acknowledge_when_durable(id, &instance_id)?;
        if let Some(bytes) = capture_bytes {
            self.observe_terminal(id, bytes)?;
        }
        if output.exited && saved.pane.state != PaneState::Dead {
            // The supervisor records the status just after it marks the process exited,
            // so a status that is not there yet stays unknown rather than guessed.
            let exit_code = self.exit_code_of(&instance_id)?;
            let mut dead = saved;
            dead.pane.exit_code = exit_code;
            dead.pane.state = PaneState::Dead;
            dead.pane.phrase = "Shell exited".into();
            self.save_runtime(id, dead)?;
        }
        Ok(())
    }

    /// Tells the supervisor it may drop the output of pane `id` once that output is on the
    /// disk. The flush runs off the lock, one at a time per pane; output that arrives while
    /// it runs is flushed by the next one.
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
        let instance_id = instance_id.to_owned();
        if !durable.pending() {
            // Nothing was written to the disk (recording is off): nothing to wait for.
            let acknowledged = self.acknowledge(&instance_id, durable.through);
            if acknowledged.is_err() {
                self.owe_acknowledgement(id, durable.through);
            }
            return acknowledged;
        }
        let pane_id = id.to_owned();
        let durable = Arc::new(durable);
        let (waiting, applying) = (Arc::clone(&durable), Arc::clone(&durable));
        let started = self.spawn_background(
            job,
            move || waiting.flush(),
            move |core, flushed| match flushed {
                Ok(()) => {
                    let acknowledged = core.acknowledge(&instance_id, applying.through);
                    if acknowledged.is_err() {
                        // The flush is done: only the acknowledgement is owed again.
                        core.owe_acknowledgement(&pane_id, applying.through);
                    }
                    acknowledged
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

    fn acknowledge(&mut self, instance_id: &str, seq: u64) -> Result<()> {
        self.pty_request(Request::Ack {
            instance_id: instance_id.to_owned(),
            seq,
        })
        .map(|_| ())
    }

    /// The exit status the supervisor recorded for an instance, if it has recorded one.
    fn exit_code_of(&mut self, instance_id: &str) -> Result<Option<i64>> {
        let Response::Instances(instances) = self.pty_request(Request::List)? else {
            return Err(errors::ptyd_unavailable("Invalid process registry"));
        };
        Ok(instances
            .iter()
            .find(|i| i.instance_id == instance_id)
            .and_then(|i| i.exit_code)
            .map(i64::from))
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
    }

    impl SupervisorPort for Holding {
        fn request(&mut self, request: Request) -> soyeht_ptyd::Result<Response> {
            Ok(match request {
                Request::Drain { from_seq, .. } => {
                    let text = self.output.pop_front().unwrap_or("");
                    let next_seq = self.position + text.len() as u64;
                    self.position = next_seq;
                    Response::Output(Output {
                        from_seq,
                        next_seq,
                        end_seq: next_seq,
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
        core.ptyd = Some(Box::new(Holding {
            output: VecDeque::from(["hello", " world"]),
            position: 0,
            acknowledged: Arc::clone(&acknowledged),
            refuse: Arc::new(Mutex::new(refusals)),
        }));
        (core, acknowledged, root, id)
    }

    fn wait_for_acknowledgements(core: &mut Core, seen: &Mutex<Vec<u64>>, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while seen.lock().unwrap().len() < count {
            assert!(
                Instant::now() < deadline,
                "the output was never acknowledged"
            );
            core.apply_background();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn output_is_acknowledged_only_after_its_flush() {
        let (mut core, acknowledged, root, id) = core_with_output(true);

        core.drain_terminal(&id).unwrap();

        // The tick wrote the output and went on: the supervisor still holds it.
        assert!(acknowledged.lock().unwrap().is_empty());
        assert_eq!(core.panes[&id].screen.next_offset(), 5);
        wait_for_acknowledgements(&mut core, &acknowledged, 1);
        assert_eq!(*acknowledged.lock().unwrap(), vec![5]);

        // Output that arrives later is flushed and acknowledged by the next job.
        core.drain_terminal(&id).unwrap();
        wait_for_acknowledgements(&mut core, &acknowledged, 2);
        assert_eq!(*acknowledged.lock().unwrap(), vec![5, 11]);
        crate::test_paths::remove_after(core, root);
    }

    #[test]
    fn output_nobody_records_is_acknowledged_at_once() {
        let (mut core, acknowledged, root, id) = core_with_output(false);

        core.drain_terminal(&id).unwrap();

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
}
