//! What the supervisor says about its processes' output, so the engine reads a process when it has
//! some and not on a timer (ADR 0147, step 1a).
//!
//! A second connection with the watch role is opened after the request connection. A thread owns it
//! and turns each notice into a mark: the process has output up to `end_seq` that the engine has not
//! read. The thread never takes the Core lock: it takes the mutex of [`Shared`], which no one holds
//! across a call, and wakes the tick. The tick reads the marked processes only. A read clears the
//! mark when it reached what the notice announced.
//!
//! Nothing here is trusted alone. The answer to the first `Watch` lists what had output before the
//! watch (level before edge), and once a second the engine reads every process once more, counting
//! the reads that found output nobody announced. A watch that cannot be had, an older supervisor or
//! a connection that dropped, leaves the engine polling as it always did.

use soyeht_platform::{
    local::Stream,
    log::{self, Failure as _, Line},
};
use soyeht_ptyd::{
    Error,
    client::{Notice, Watcher},
    wire::Readable,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::Shutdown,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// How often every process is read once more, whatever the notices said.
pub(crate) const SWEEP: Duration = Duration::from_secs(1);
/// A sweep read that finds output no notice announced for this long is a missed notice.
const MISS_AFTER: Duration = Duration::from_secs(2);
/// How long after a watch connection ended (or failed) the engine tries again.
const RETRY: Duration = Duration::from_secs(2);
/// How many reads in a row a truncated read may start at once, with no wait for the tick, before
/// the process waits for the regular tick like the others.
pub(crate) const IMMEDIATE_REPEATS: u8 = 16;
/// After a read that found output and reached the end of what the supervisor held, a tick woken by
/// a notice leaves the process alone this long (one tick): the regular tick reads what arrived
/// meanwhile, so a process that writes in small chunks costs a read and an apply per tick and not
/// one per round trip.
const PACE: Duration = Duration::from_millis(10);

#[derive(Clone, Copy)]
struct Mark {
    /// The end of what the supervisor said it holds; a read that reaches it clears the mark.
    end_seq: u64,
    /// The process exited and has no more: a read that sees the exit clears the mark.
    exited: bool,
    /// Set by a sweep, not by a notice.
    swept: bool,
}

/// What the tick needs to know about a read that ended.
pub(crate) struct ReadSummary {
    pub(crate) next_seq: u64,
    /// The end of what the supervisor held when the read was made.
    pub(crate) end_seq: u64,
    pub(crate) exited: bool,
    pub(crate) found: bool,
}

struct Inner {
    /// Changes whenever a watch is started or stopped; a thread that finds it changed is stale.
    epoch: u64,
    active: bool,
    running: bool,
    /// The supervisor of this link does not know the role: not tried again until the link changes.
    unsupported: bool,
    retry_at: Option<Instant>,
    marks: BTreeMap<String, Mark>,
    last_notice: BTreeMap<String, Instant>,
    repeats: BTreeMap<String, u8>,
    /// When a woken tick may read a process again after a read that found output.
    paced: BTreeMap<String, Instant>,
    last_sweep: Option<Instant>,
    missed: u64,
    closer: Option<Stream>,
    failures: crate::streak::Streak,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            epoch: 0,
            active: false,
            running: false,
            unsupported: false,
            retry_at: None,
            marks: BTreeMap::new(),
            last_notice: BTreeMap::new(),
            repeats: BTreeMap::new(),
            paced: BTreeMap::new(),
            last_sweep: None,
            missed: 0,
            closer: None,
            failures: crate::streak::Streak::once(),
        }
    }
}

#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
}

impl Shared {
    fn with<T>(&self, change: impl FnOnce(&mut Inner) -> T) -> T {
        change(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// The engine's end of the watch.
#[derive(Default)]
pub(crate) struct Watch {
    shared: Arc<Shared>,
    /// Set when a link to the supervisor was made by the product's connection code: a link a test
    /// plants in the Core is never watched.
    wanted: bool,
}

impl Watch {
    /// A link was made: watch it. Any watch of the link before is over.
    pub(crate) fn link_made(&mut self) {
        self.stop();
        self.wanted = true;
    }

    /// The link is gone, or about to be replaced: the watch of it is over.
    pub(crate) fn stop(&mut self) {
        self.wanted = false;
        self.shared.with(|inner| {
            inner.epoch += 1;
            inner.active = false;
            inner.running = false;
            inner.unsupported = false;
            inner.retry_at = None;
            inner.marks.clear();
            inner.repeats.clear();
            inner.paced.clear();
            if let Some(stream) = inner.closer.take() {
                // ignored: the thread may have ended already; either way it is stale now.
                let _ = stream.shutdown(Shutdown::Both);
            }
        });
    }

    /// Whether notices are being heard, so a process is read when it has output.
    pub(crate) fn is_active(&self) -> bool {
        self.shared.with(|inner| inner.active)
    }

    /// Whether `instance_id` has output the engine has not read.
    pub(crate) fn is_marked(&self, instance_id: &str) -> bool {
        self.shared
            .with(|inner| inner.active && inner.marks.contains_key(instance_id))
    }

    /// Whether the tick should read `instance_id` now. A tick woken by a notice reads a process
    /// that keeps truncating at once, up to [`IMMEDIATE_REPEATS`] times in a row, and leaves one
    /// that has just been read to the end for [`PACE`]; the regular tick always may.
    pub(crate) fn should_read(&self, instance_id: &str, woken: bool) -> bool {
        self.shared.with(|inner| {
            inner.marks.contains_key(instance_id)
                && (!woken
                    || (inner.repeats.get(instance_id).copied().unwrap_or(0) < IMMEDIATE_REPEATS
                        && inner
                            .paced
                            .get(instance_id)
                            .is_none_or(|until| Instant::now() >= *until)))
        })
    }

    /// A read of `instance_id` ended and was applied.
    pub(crate) fn note_read(&self, instance_id: &str, read: &ReadSummary) {
        let missed = self.shared.with(|inner| {
            let mut mark = inner.marks.get(instance_id).copied()?;
            let mut missed = false;
            if mark.swept && read.found {
                let announced = inner
                    .last_notice
                    .get(instance_id)
                    .is_some_and(|at| at.elapsed() < MISS_AFTER);
                if !announced {
                    inner.missed += 1;
                    missed = inner.failures.fail("missed_notice");
                }
                // It is known now how far there is to read: the mark is no sweep's any more, so
                // a read that stops short of it leaves it, and is not counted again.
                mark.swept = false;
                mark.end_seq = read.end_seq;
                inner.marks.insert(instance_id.to_owned(), mark);
            }
            // The mark of a sweep waits for what the supervisor held when it was read; the mark of
            // a notice also for what the notice announced, and for what a later one did.
            let target = if mark.swept {
                read.end_seq
            } else {
                mark.end_seq.max(read.end_seq)
            };
            let reached = read.next_seq >= target;
            if reached && (!mark.exited || read.exited) {
                inner.marks.remove(instance_id);
                inner.repeats.remove(instance_id);
                if read.found {
                    inner
                        .paced
                        .insert(instance_id.to_owned(), Instant::now() + PACE);
                }
            } else {
                if !reached {
                    // More is waiting: the next read is not paced.
                    inner.paced.remove(instance_id);
                }
                // Capped: a process that stays behind for thousands of reads must not overflow it.
                let repeats = inner.repeats.entry(instance_id.to_owned()).or_insert(0);
                *repeats = repeats.saturating_add(1).min(IMMEDIATE_REPEATS);
            }
            Some(missed)
        });
        if missed == Some(true) {
            Line::new("engine.supervisor_watch_missed").emit();
        }
    }

    /// When the tick must act on the watch: the next sweep while it is active and a process is
    /// live, and the next try of a watch that is wanted and not running. Notices wake the tick.
    pub(crate) fn next_due(&self, live: bool) -> Option<Duration> {
        self.shared.with(|inner| {
            if inner.active {
                let since = inner.last_sweep.map_or(SWEEP, |at| at.elapsed());
                return live.then(|| SWEEP.saturating_sub(since));
            }
            if !self.wanted || inner.running || inner.unsupported {
                return None;
            }
            Some(inner.retry_at.map_or(Duration::ZERO, |at| {
                at.saturating_duration_since(Instant::now())
            }))
        })
    }

    /// Marks every process in `instances` for a read when a second has passed since the last
    /// sweep. Returns whether it did.
    pub(crate) fn sweep<'a>(&self, instances: impl Iterator<Item = &'a str>) -> bool {
        self.shared.with(|inner| {
            if !inner.active || inner.last_sweep.is_some_and(|at| at.elapsed() < SWEEP) {
                return false;
            }
            inner.last_sweep = Some(Instant::now());
            let live: BTreeSet<&str> = instances.collect();
            // What belongs to a process that is gone goes with it.
            inner.marks.retain(|id, _| live.contains(id.as_str()));
            inner.repeats.retain(|id, _| live.contains(id.as_str()));
            inner.paced.retain(|id, _| live.contains(id.as_str()));
            inner.last_notice.retain(|id, _| live.contains(id.as_str()));
            for instance_id in live {
                inner.marks.entry(instance_id.to_owned()).or_insert(Mark {
                    end_seq: u64::MAX,
                    exited: false,
                    swept: true,
                });
            }
            true
        })
    }

    /// Starts the watch of the link when it is wanted, is not running, and is not known to be
    /// unsupported or waiting to be retried. The connection is made by a thread of its own: it can
    /// wait for the supervisor's welcome.
    pub(crate) fn keep(
        &mut self,
        socket: PathBuf,
        generation: u64,
        woken: Arc<tokio::sync::Notify>,
    ) {
        let epoch = self.shared.with(|inner| {
            let due = inner.retry_at.is_none_or(|at| Instant::now() >= at);
            if !self.wanted || inner.running || inner.unsupported || !due {
                return None;
            }
            inner.running = true;
            Some(inner.epoch)
        });
        if let Some(epoch) = epoch {
            let shared = Arc::clone(&self.shared);
            thread::spawn(move || run(&shared, epoch, &socket, generation, &woken));
        }
    }

    /// Whether the supervisor of this link was found not to know the role.
    #[cfg(test)]
    pub(crate) fn is_unsupported(&self) -> bool {
        self.shared.with(|inner| inner.unsupported)
    }

    /// The notices that were not announced, for tests and the log.
    #[cfg(test)]
    pub(crate) fn missed(&self) -> u64 {
        self.shared.with(|inner| inner.missed)
    }

    /// Makes the watch active with nothing marked, as if the supervisor had answered.
    #[cfg(test)]
    pub(crate) fn pretend_active(&self) {
        self.shared.with(|inner| inner.active = true);
    }

    /// Marks `instance_id` as a notice would.
    #[cfg(test)]
    pub(crate) fn pretend_notice(&self, instance_id: &str, end_seq: u64) {
        self.shared
            .with(|inner| mark(inner, instance_id, end_seq, false));
    }
}

impl crate::core::Core {
    /// Once a second, marks every pane's process for a read whatever the notices said, so a notice
    /// that never came costs a second and not the output.
    pub(crate) fn sweep_watch(&mut self) {
        let servers = crate::languages::watching::instances(self);
        let instances = self
            .panes
            .values()
            .filter(|pane| !pane.stored.closed)
            .filter_map(|pane| pane.stored.instance_id.as_deref())
            .chain(servers.iter().map(String::as_str));
        self.watch.sweep(instances);
    }

    /// Asks the watch to start, or to try again, when the link is up.
    pub(crate) fn keep_watch(&mut self) {
        let socket = self.directory.join(soyeht_protocol::profile::PTYD_SOCKET);
        self.watch
            .keep(socket, self.generation, self.background.woken());
    }

    /// When the tick must read the processes of the panes: now for a pane the supervisor said
    /// has output, for an agent that is not at rest and for every pane while no watch is heard
    /// (the engine polls then, as it always did); otherwise at the next sweep or the next
    /// deadline of an agent.
    pub(crate) fn output_due(&self, next: &mut crate::tick_due::Next, now: i64) {
        let mut live = false;
        for (id, pane) in &self.panes {
            if pane.stored.closed
                || pane.stored.instance_id.is_none()
                || self.capture_restore_pending(id)
            {
                continue;
            }
            live = true;
            if self.agents.contains_key(id) {
                self.agent_poll_due(id, next, now);
            } else if !self.watch.is_active() || self.pane_marked(id) {
                next.now();
            }
        }
        if let Some(wait) = self.watch.next_due(live) {
            next.within(wait);
        }
    }

    /// Whether the supervisor said the process of pane `id` has output to read.
    pub(crate) fn pane_marked(&self, id: &str) -> bool {
        self.panes
            .get(id)
            .and_then(|pane| pane.stored.instance_id.as_deref())
            .is_some_and(|instance_id| self.watch.is_marked(instance_id))
    }
}

fn mark(inner: &mut Inner, instance_id: &str, end_seq: u64, exited: bool) {
    inner
        .last_notice
        .insert(instance_id.to_owned(), Instant::now());
    let entry = inner.marks.entry(instance_id.to_owned()).or_insert(Mark {
        end_seq,
        exited,
        swept: false,
    });
    // A notice replaces a sweep's mark, and a newer notice raises the end it waits for.
    entry.swept = false;
    entry.end_seq = if entry.end_seq == u64::MAX {
        end_seq
    } else {
        entry.end_seq.max(end_seq)
    };
    entry.exited |= exited;
}

fn run(
    shared: &Shared,
    epoch: u64,
    socket: &std::path::Path,
    generation: u64,
    woken: &tokio::sync::Notify,
) {
    let outcome = watch(shared, epoch, socket, generation, woken);
    // What to write is decided under the mutex and written after it.
    let log = shared.with(|inner| {
        if inner.epoch != epoch {
            return None;
        }
        inner.active = false;
        inner.running = false;
        inner.closer = None;
        inner.retry_at = Some(Instant::now() + RETRY);
        inner.marks.clear();
        match outcome {
            Err(Error::WatchUnavailable) => {
                inner.unsupported = true;
                Some(None)
            }
            Err(error) => inner
                .failures
                .fail(&error.failure_kind())
                .then_some(Some(error)),
            Ok(()) => None,
        }
    });
    match log {
        Some(None) => Line::new("engine.supervisor_watch_unavailable").emit(),
        Some(Some(error)) => log::failure("engine.supervisor_watch_failed", &error),
        None => {}
    }
    // Polling resumes on the next tick.
    woken.notify_one();
}

fn watch(
    shared: &Shared,
    epoch: u64,
    socket: &std::path::Path,
    generation: u64,
    woken: &tokio::sync::Notify,
) -> Result<(), Error> {
    let mut watcher = Watcher::connect(socket, generation)?;
    let closer = watcher.stream().try_clone().map_err(Error::Io)?;
    let current = shared.with(|inner| {
        let current = inner.epoch == epoch;
        if current {
            inner.closer = Some(closer);
        }
        current
    });
    if !current {
        return Ok(());
    }
    // Nothing named: every process is one the engine has read nothing of, so each that has output
    // comes back marked.
    let level = watcher.watch(Vec::new())?;
    let current = shared.with(|inner| {
        if inner.epoch != epoch {
            return false;
        }
        for Readable {
            instance_id,
            end_seq,
            exited,
        } in &level
        {
            mark(inner, instance_id, *end_seq, *exited);
        }
        inner.active = true;
        inner.failures.succeed();
        true
    });
    woken.notify_one();
    if !current {
        return Ok(());
    }
    loop {
        match watcher.notice()? {
            Notice::Readable(Readable {
                instance_id,
                end_seq,
                exited,
            }) => {
                let current = shared.with(|inner| {
                    let current = inner.epoch == epoch;
                    if current {
                        mark(inner, &instance_id, end_seq, exited);
                    }
                    current
                });
                if !current {
                    return Ok(());
                }
                woken.notify_one();
            }
            Notice::Level(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(next_seq: u64, end_seq: u64, exited: bool, found: bool) -> ReadSummary {
        ReadSummary {
            next_seq,
            end_seq,
            exited,
            found,
        }
    }

    #[test]
    fn a_read_that_reaches_what_was_announced_clears_the_mark_and_a_short_one_keeps_it() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", 100_000);
        assert!(watch.is_marked("p"));
        watch.note_read("p", &read(65_536, 100_000, false, true));
        assert!(
            watch.is_marked("p"),
            "a truncated read leaves the rest to read"
        );
        watch.note_read("p", &read(100_000, 100_000, false, true));
        assert!(!watch.is_marked("p"));
    }

    #[test]
    fn output_that_arrives_during_a_read_keeps_the_mark() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", 10);
        watch.pretend_notice("p", 30);
        watch.note_read("p", &read(10, 10, false, true));
        assert!(watch.is_marked("p"));
        watch.note_read("p", &read(30, 30, false, true));
        assert!(!watch.is_marked("p"));
    }

    #[test]
    fn a_read_that_stops_short_of_what_the_supervisor_holds_keeps_the_mark_of_a_notice() {
        // The first notice of a burst said 8; the read found 400063 held and returned 64 KiB.
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", 8);
        watch.note_read("p", &read(65_536, 400_063, false, true));
        assert!(
            watch.is_marked("p"),
            "the rest of the burst is still to read"
        );
        // The next read is not paced, and the one that reaches the end clears the mark.
        assert!(watch.should_read("p", true));
        watch.note_read("p", &read(400_063, 400_063, false, true));
        assert!(!watch.is_marked("p"));
    }

    #[test]
    fn a_read_that_stops_short_keeps_the_mark_of_a_sweep_and_counts_the_miss_once() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.sweep(["p"].into_iter());
        watch.note_read("p", &read(65_536, 400_063, false, true));
        assert!(watch.is_marked("p"));
        assert_eq!(watch.missed(), 1);
        watch.note_read("p", &read(131_072, 400_063, false, true));
        assert_eq!(watch.missed(), 1, "the same burst is one miss");
        watch.note_read("p", &read(400_063, 400_063, false, true));
        assert!(!watch.is_marked("p"));
    }

    #[test]
    fn a_woken_tick_leaves_a_process_it_just_read_to_the_end_for_one_tick() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", 5);
        watch.note_read("p", &read(5, 5, false, true));
        // More output arrives at once: the regular tick reads it, the woken one waits.
        watch.pretend_notice("p", 9);
        assert!(!watch.should_read("p", true));
        assert!(watch.should_read("p", false));
        std::thread::sleep(PACE * 2);
        assert!(watch.should_read("p", true));
        // A read that found nothing does not pace the next.
        watch.note_read("p", &read(9, 9, false, false));
        watch.pretend_notice("p", 12);
        assert!(watch.should_read("p", true));
    }

    #[test]
    fn an_exit_stays_marked_until_a_read_sees_it() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.shared.with(|inner| mark(inner, "p", 5, true));
        watch.note_read("p", &read(5, 5, false, true));
        assert!(watch.is_marked("p"));
        watch.note_read("p", &read(5, 5, true, false));
        assert!(!watch.is_marked("p"));
    }

    #[test]
    fn a_process_that_stays_behind_for_thousands_of_reads_keeps_a_bounded_count() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", u64::MAX - 1);
        // 64 KiB a read: a few hundred reads are some tens of megabytes of one build log.
        for _ in 0..5_000 {
            watch.note_read("p", &read(10, u64::MAX - 1, false, true));
        }
        assert!(watch.is_marked("p"));
        assert!(
            !watch.should_read("p", true),
            "a woken tick still leaves it to the regular one"
        );
        assert!(watch.should_read("p", false));
        // The read that reaches the end clears the count with the mark.
        watch.note_read("p", &read(u64::MAX - 1, u64::MAX - 1, false, false));
        assert!(!watch.is_marked("p"));
        watch.pretend_notice("p", 5);
        assert!(
            watch.should_read("p", true),
            "the count did not outlive the mark"
        );
    }

    #[test]
    fn a_pane_that_keeps_truncating_waits_for_the_regular_tick_after_sixteen_repeats() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", u64::MAX - 1);
        for _ in 0..IMMEDIATE_REPEATS {
            assert!(watch.should_read("p", true));
            watch.note_read("p", &read(10, u64::MAX - 1, false, true));
        }
        assert!(
            !watch.should_read("p", true),
            "a woken tick leaves it to the regular one"
        );
        assert!(watch.should_read("p", false));
    }

    #[test]
    fn a_sweep_marks_every_process_once_a_second_and_counts_output_nobody_announced() {
        let watch = Watch::default();
        watch.pretend_active();
        assert!(watch.sweep(["a", "b"].into_iter()));
        assert!(
            !watch.sweep(["a", "b"].into_iter()),
            "not twice in a second"
        );
        assert!(watch.is_marked("a") && watch.is_marked("b"));
        // A sweep read that finds nothing clears the mark and counts nothing.
        watch.note_read("a", &read(0, 0, false, false));
        assert!(!watch.is_marked("a"));
        assert_eq!(watch.missed(), 0);
        // One that finds output no notice announced is a miss.
        watch.note_read("b", &read(7, 7, false, true));
        assert_eq!(watch.missed(), 1);
    }

    #[test]
    fn output_a_notice_announced_is_not_a_miss() {
        let watch = Watch::default();
        watch.pretend_active();
        watch.pretend_notice("p", 7);
        watch.shared.with(|inner| {
            inner.marks.insert(
                "p".into(),
                Mark {
                    end_seq: u64::MAX,
                    exited: false,
                    swept: true,
                },
            );
        });
        watch.note_read("p", &read(7, 7, false, true));
        assert_eq!(watch.missed(), 0);
    }

    #[test]
    fn nothing_is_read_for_a_mark_when_the_watch_is_not_active() {
        let watch = Watch::default();
        watch.pretend_notice("p", 7);
        assert!(!watch.is_active());
        assert!(!watch.is_marked("p"));
    }

    #[test]
    fn a_supervisor_from_before_the_watch_leaves_the_engine_polling() {
        let (mut core, directory, id, _calls) =
            crate::supervisor_link::tests::core_with_unstarted_pane(Vec::new());
        core.panes.remove(&id);
        core.lose_supervisor_link();
        // This supervisor welcomes every hello with no features, as the one before the role does.
        crate::supervisor_link::tests::serve_registry(&directory);
        core.background = crate::offload::Background::inline();
        assert_eq!(core.ensure_supervisor(), crate::supervisor_link::Link::Up);

        // The next tick asks for a watch, and learns it cannot have one.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !core.watch.is_unsupported() {
            assert!(
                std::time::Instant::now() < deadline,
                "the watch never gave up"
            );
            assert_eq!(core.ensure_supervisor(), crate::supervisor_link::Link::Up);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!core.watch.is_active(), "the panes are read on every tick");

        // A new link is asked again: this one may be a newer supervisor.
        core.lose_supervisor_link();
        assert!(!core.watch.is_unsupported());
    }
}
