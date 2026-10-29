//! Reads of a process's output that wait for the supervisor off the Core lock, for the polls that
//! consume them (ADR 0117, ADR 0119).
//!
//! A shell pane applies its read straight into its screen (`terminal_drain`). The polls of an
//! agent, of its inline terminals and of a language server hold state machines that are too long
//! to run when a job ends, and that order their steps against the rest of the tick. They keep
//! their place in the tick instead: a read is started with no lock and, when it ends, the tick
//! applies it by putting it in this table; the poll takes it from the table, and starts the
//! next read as soon as it knows where that read begins. A poll that finds no read for the
//! offset it needs does nothing this tick.
//!
//! The supervisor keeps what it has not been told to drop, so a read that is never taken (the
//! process was replaced, the pane closed, the screen moved) costs nothing: the poll that wants
//! another offset or another process discards it and asks again.

use crate::{
    core::Core,
    errors::Result,
    supervisor_handle::Supervisor,
    terminal_drain::{Read, read_output},
};
use std::collections::BTreeMap;

/// The read a job ended with, and what it asked for.
pub(crate) struct Ready {
    instance_id: String,
    from_seq: u64,
    read: Result<Read>,
}

/// How long an idle agent that said nothing is left alone before it is read again. An agent
/// that has nothing to advance (ADR 0080) waits for a person or for a request to expire, and the
/// supervisor keeps what it says meanwhile, so asking at the pace of the tick only costs the
/// engine a request, a wake of the supervisor and two hand-offs between threads for nothing. A
/// change of the agent's state ends the wait at once.
pub(crate) const IDLE_PACE_MS: i64 = 100;

/// The reads that ended and have not been taken, by the owner that asked, and the owners that
/// are left alone until some time.
#[derive(Default)]
pub(crate) struct Reads {
    ready: BTreeMap<String, Ready>,
    paused_until_ms: BTreeMap<String, i64>,
}

/// The owner of the reads of an agent's own process.
pub(crate) fn agent(pane: &str) -> String {
    format!("agent:{pane}")
}

/// The owner of the reads of the inline terminal `terminal` of an agent.
pub(crate) fn agent_terminal(pane: &str, terminal: &str) -> String {
    format!("agent:{pane}:terminal:{terminal}")
}

/// The owner of the reads of the language server `key`.
pub(crate) fn language(key: &str) -> String {
    format!("language:{key}")
}

/// The job that reads for `owner`. One at a time per owner.
fn job(owner: &str) -> String {
    format!("read:{owner}")
}

impl Core {
    /// Starts a read of `instance_id` from `from_seq` for `owner`, unless one is running for it.
    /// With `look_up_exit` the read also looks up the exit status when the process exited.
    pub(crate) fn start_read(
        &mut self,
        owner: &str,
        instance_id: &str,
        from_seq: u64,
        look_up_exit: bool,
    ) -> Result<()> {
        let job = job(owner);
        if !self.background.accepts(&job) {
            return Ok(());
        }
        let supervisor = self.supervisor()?;
        let used: Supervisor = supervisor.clone();
        let (owner, instance_id) = (owner.to_owned(), instance_id.to_owned());
        let asked = instance_id.clone();
        self.spawn_background(
            job,
            move || read_output(&supervisor, &asked, from_seq, look_up_exit),
            move |core, read| {
                // A failed read on a link that was replaced meanwhile says nothing about the new
                // one: it is dropped.
                let settled = match core.settle_if_current(&used, read) {
                    Ok(Some(read)) => Ok(read),
                    Ok(None) => return Ok(()),
                    Err(error) => Err(error),
                };
                core.reads.ready.insert(
                    owner,
                    Ready {
                        instance_id,
                        from_seq,
                        read: settled,
                    },
                );
                Ok(())
            },
        )
    }

    /// The read `owner` asked for, if it ended: the answer for `instance_id` from `from_seq`. With
    /// none, a read is started and `None` is returned (the owner polls again next tick); a read
    /// that was asked for another place is discarded.
    pub(crate) fn take_read(
        &mut self,
        owner: &str,
        instance_id: &str,
        from_seq: u64,
        look_up_exit: bool,
    ) -> Result<Option<Read>> {
        if let Some(read) = self.taken(owner, instance_id, from_seq) {
            return read.map(Some);
        }
        self.start_read(owner, instance_id, from_seq, look_up_exit)?;
        // A read that ends in the caller (unit tests run their jobs there) is ready now.
        self.taken(owner, instance_id, from_seq).transpose()
    }

    fn taken(&mut self, owner: &str, instance_id: &str, from_seq: u64) -> Option<Result<Read>> {
        let ready = self.reads.ready.remove(owner)?;
        (ready.instance_id == instance_id && ready.from_seq == from_seq).then_some(ready.read)
    }

    /// Forgets the reads of the owners that start with `prefix`: their pane or server is gone.
    pub(crate) fn forget_reads(&mut self, prefix: &str) {
        self.reads
            .ready
            .retain(|owner, _| !owner.starts_with(prefix));
        self.reads
            .paused_until_ms
            .retain(|owner, _| !owner.starts_with(prefix));
    }

    /// Leaves `owner` alone for [`IDLE_PACE_MS`]: its next read starts after that, so a quiet
    /// idle process is read at a slower pace than the tick.
    pub(crate) fn pause_reads(&mut self, owner: &str) {
        let until = self.now_ms().saturating_add(IDLE_PACE_MS);
        self.reads.paused_until_ms.insert(owner.to_owned(), until);
    }

    /// Whether `owner` is being left alone.
    pub(crate) fn reads_paused(&self, owner: &str) -> bool {
        self.reads
            .paused_until_ms
            .get(owner)
            .is_some_and(|until| self.now_ms() < *until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_panes::{Answering, quiet_read};
    use soyeht_ptyd::wire::{Request, Response};
    use std::{
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    fn core_with(delay: Duration) -> (Core, std::path::PathBuf, Arc<Mutex<Vec<u64>>>) {
        let directory = crate::supervisor_link::tests::short_directory();
        let mut core = Core::open(&directory).unwrap();
        // A supervisor with no output that records the offsets it was asked for and takes
        // `delay` to answer.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&asked);
        core.link_supervisor(Box::new(Answering(move |request| {
            let Request::Drain { from_seq, .. } = request else {
                return Ok(Response::Ok);
            };
            record.lock().unwrap().push(from_seq);
            thread::sleep(delay);
            Ok(quiet_read(from_seq))
        })));
        (core, directory, asked)
    }

    #[test]
    fn a_read_waits_off_the_lock_and_the_poll_takes_it_after_the_tick_applied_it() {
        let (mut core, directory, asked) = core_with(Duration::from_millis(300));

        let started = Instant::now();
        assert!(
            core.take_read("owner", "process", 7, false)
                .unwrap()
                .is_none()
        );
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "the poll waited for the supervisor"
        );
        // While the read is running, the poll asks for nothing more.
        assert!(
            core.take_read("owner", "process", 7, false)
                .unwrap()
                .is_none()
        );

        let deadline = Instant::now() + Duration::from_secs(30);
        while core.background.is_active("read:owner") {
            assert!(Instant::now() < deadline, "the read never ended");
            core.apply_background();
            thread::sleep(Duration::from_millis(1));
        }
        let read = core.take_read("owner", "process", 7, false).unwrap();
        assert_eq!(read.unwrap().output.from_seq, 7);
        assert_eq!(*asked.lock().unwrap(), [7]);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_read_asked_for_another_place_is_discarded_and_asked_again() {
        let (mut core, directory, asked) = core_with(Duration::ZERO);
        core.background = crate::offload::Background::inline();

        core.start_read("owner", "process", 5, false).unwrap();
        // The screen moved, then the process was replaced: neither read fits.
        let moved = core.take_read("owner", "process", 9, false).unwrap();
        assert_eq!(moved.unwrap().output.from_seq, 9);
        core.start_read("owner", "process", 9, false).unwrap();
        let replaced = core.take_read("owner", "other", 9, false).unwrap();
        assert_eq!(replaced.unwrap().output.from_seq, 9);

        assert_eq!(*asked.lock().unwrap(), [5, 9, 9, 9]);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn the_reads_of_a_closed_pane_are_forgotten() {
        let (mut core, directory, asked) = core_with(Duration::ZERO);
        core.background = crate::offload::Background::inline();
        core.start_read(&agent("pane"), "process", 1, false)
            .unwrap();
        core.start_read(&agent_terminal("pane", "t"), "child", 1, true)
            .unwrap();
        core.start_read(&agent("other"), "process", 1, false)
            .unwrap();

        core.forget_reads(&agent("pane"));

        // Only the read of the pane that is still open waits to be taken.
        let before = asked.lock().unwrap().len();
        assert!(
            core.take_read(&agent("other"), "process", 1, false)
                .unwrap()
                .is_some()
        );
        assert_eq!(asked.lock().unwrap().len(), before);
        assert!(
            core.take_read(&agent("pane"), "process", 1, false)
                .unwrap()
                .is_some()
        );
        assert_eq!(asked.lock().unwrap().len(), before + 1);
        crate::test_paths::remove_after(core, directory);
    }
}
