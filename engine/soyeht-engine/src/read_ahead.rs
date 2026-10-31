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
use std::collections::{BTreeMap, BTreeSet};

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

/// The reads that ended and have not been taken, by the owner that asked, the owners that are
/// left alone until some time, and the owners that are still asking: a read that ends for an
/// owner that is gone (its pane closed, its link lost) is dropped.
#[derive(Default)]
pub(crate) struct Reads {
    ready: BTreeMap<String, Ready>,
    paused_until_ms: BTreeMap<String, i64>,
    live: BTreeSet<String>,
    /// Set while the tick polls what a read woke: the polls take the reads that ended and start
    /// none, and leave the panes that are not agents to the regular tick.
    polling_woken: bool,
    /// What the jobs that read the requests an agent had pending found, by pane, until its poll
    /// takes it (`agents/recovery.rs`).
    pub(crate) fetches: BTreeMap<String, crate::agents::recovery::Fetched>,
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

/// Whether a read found something the poll should see at once: output, a gap, or an error (a
/// lost link is dropped when its error is applied). A quiet read, or an exit with nothing left
/// to read, waits for the next tick, so an idle process never makes the tick spin.
fn has_news(read: &Result<Read>) -> bool {
    !read
        .as_ref()
        .is_ok_and(|read| read.output.bytes.is_empty() && read.output.gap.is_none())
}

/// Whether a read for `owner` may wake the tick. A language server's read does not: nothing the
/// woken tick polls is its.
fn wakes(owner: &str) -> bool {
    !owner.starts_with("language:")
}

/// A read that never wakes the tick.
fn quiet(_: &Result<Read>) -> bool {
    false
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
        if self.reads.polling_woken || !self.background.accepts(&job) {
            return Ok(());
        }
        let supervisor = self.supervisor()?;
        let used: Supervisor = supervisor.clone();
        self.reads.live.insert(owner.to_owned());
        let prompt: fn(&Result<Read>) -> bool = if wakes(owner) { has_news } else { quiet };
        let (owner, instance_id) = (owner.to_owned(), instance_id.to_owned());
        let asked = instance_id.clone();
        self.spawn_prompt_background(
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
                if !core.reads.live.contains(&owner) {
                    return Ok(());
                }
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
            prompt,
        )
    }

    /// The tick's part when a read brought news: the agents take what it found at once, so
    /// output is not left waiting for the next tick (see [`has_news`]). It starts no read: the
    /// next one starts on the regular tick, so a process that never stops writing is polled at
    /// the pace of the tick, not at the pace of the round trip.
    pub(crate) fn poll_woken(&mut self) -> Result<()> {
        if self.ptyd.is_none() {
            return Ok(());
        }
        self.reads.polling_woken = true;
        let polled = self.drain_panes();
        self.reads.polling_woken = false;
        polled
    }

    /// Whether the tick is polling what a read woke (see [`Core::poll_woken`]).
    pub(crate) fn polling_woken(&self) -> bool {
        self.reads.polling_woken
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
        // The owner itself and the ones below it (`agent:{pane}:terminal:{id}`), not another
        // owner whose name starts the same way (the terminal `10` is not below the terminal `1`).
        let below = format!("{prefix}:");
        let owned = |owner: &str| owner == prefix || owner.starts_with(&below);
        self.reads.ready.retain(|owner, _| !owned(owner));
        self.reads.paused_until_ms.retain(|owner, _| !owned(owner));
        self.reads.live.retain(|owner| !owned(owner));
        self.reads.fetches.retain(|owner, _| !owned(owner));
    }

    /// Forgets every read: the link they were made on is gone.
    pub(crate) fn forget_all_reads(&mut self) {
        self.reads = Reads::default();
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
    use crate::test_panes::{Answering, apply_until_done, quiet_read};
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

    /// Whether the read started for `owner` (at offset 1) was kept: it is taken with no new
    /// request to the supervisor.
    fn kept(core: &mut Core, asked: &Mutex<Vec<u64>>, owner: &str) -> bool {
        let before = asked.lock().unwrap().len();
        assert!(core.take_read(owner, "process", 1, true).unwrap().is_some());
        asked.lock().unwrap().len() == before
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

        apply_until_done(&mut core, "read:owner");
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
        assert!(kept(&mut core, &asked, &agent("other")));
        assert!(!kept(&mut core, &asked, &agent("pane")));
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_read_that_ends_after_its_owner_was_forgotten_is_dropped() {
        let (mut core, directory, _) = core_with(Duration::from_millis(200));
        core.start_read(&agent("pane"), "process", 1, false)
            .unwrap();
        core.forget_reads(&agent("pane"));

        apply_until_done(&mut core, "read:agent:pane");
        assert!(
            core.reads.ready.is_empty(),
            "a read of a closed pane was kept"
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_lost_link_forgets_the_reads_made_on_it() {
        let (mut core, directory, _) = core_with(Duration::ZERO);
        core.background = crate::offload::Background::inline();
        core.start_read(&agent("pane"), "process", 1, false)
            .unwrap();
        assert!(!core.reads.ready.is_empty());

        core.lose_supervisor_link();

        assert!(core.reads.ready.is_empty());
        crate::test_paths::remove_after(core, directory);
    }

    /// A core with an agent pane and a shell pane, over a supervisor that has nothing to say and
    /// counts the reads it is asked for.
    fn core_with_two_panes() -> (Core, std::path::PathBuf, Arc<Mutex<u64>>) {
        let (mut core, directory) = crate::test_panes::core_with_agent(&[]);
        crate::test_panes::insert_terminal_pane(&mut core, &directory, "shell", false);
        let reads = Arc::new(Mutex::new(0));
        let count = Arc::clone(&reads);
        core.link_supervisor(Box::new(Answering(move |request| {
            Ok(match request {
                Request::Drain { from_seq, .. } => {
                    *count.lock().unwrap() += 1;
                    quiet_read(from_seq)
                }
                _ => Response::Ok,
            })
        })));
        (core, directory, reads)
    }

    #[test]
    fn a_woken_tick_takes_what_ended_and_starts_no_read_so_a_flood_is_polled_at_the_tick() {
        let (mut core, directory, reads) = core_with_two_panes();
        core.background = crate::offload::Background::inline();

        // The regular tick starts the reads of the agent and of the shell.
        core.drain_panes().unwrap();
        let started = *reads.lock().unwrap();
        assert!(started >= 2, "the tick started {started} reads");

        // A wake storm takes the read that ended and starts nothing, and leaves the shell to
        // the tick: however often a read wakes the tick, the supervisor is asked no more.
        for _ in 0..50 {
            core.poll_woken().unwrap();
        }
        assert_eq!(*reads.lock().unwrap(), started);

        // The next regular tick starts them again.
        core.drain_panes().unwrap();
        assert!(*reads.lock().unwrap() > started);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn only_the_reads_of_an_agent_wake_the_tick() {
        assert!(wakes(&agent("pane")));
        assert!(wakes(&agent_terminal("pane", "t")));
        assert!(!wakes(&language("rust")));
    }

    #[test]
    fn forgetting_the_reads_of_a_terminal_leaves_the_terminal_whose_id_starts_the_same() {
        let (mut core, directory, asked) = core_with(Duration::ZERO);
        core.background = crate::offload::Background::inline();
        core.start_read(&agent_terminal("pane", "1"), "process", 1, true)
            .unwrap();
        core.start_read(&agent_terminal("pane", "10"), "process", 1, true)
            .unwrap();

        core.forget_reads(&agent_terminal("pane", "1"));

        // The read of the terminal `10` is still there; the one of `1` is asked for again.
        assert!(kept(&mut core, &asked, &agent_terminal("pane", "10")));
        assert!(!kept(&mut core, &asked, &agent_terminal("pane", "1")));
        crate::test_paths::remove_after(core, directory);
    }
}
