//! What the two file followers (the project panel and the skill folders) share: the
//! operating-system event stream of one folder, and the timing that turns a burst of events
//! into one read.

use crate::streak::{Streak, first_failure, recovered};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::{
        Mutex,
        mpsc::{self, Receiver},
    },
};

/// A burst is read once no event arrived for this long ...
pub(crate) const QUIET_MS: i64 = 75;
/// ... or once it has lasted this long, so a steady stream still shows up.
pub(crate) const BURST_LIMIT_MS: i64 = 500;

/// Makes starting an event stream slow, as the system's file-event service is on a busy
/// machine, so a test can show what waits for it. Compiled only with `test-hooks`.
#[cfg(feature = "test-hooks")]
pub mod slow_start {
    use std::sync::atomic::{AtomicU64, Ordering};

    static PAUSE_MS: AtomicU64 = AtomicU64::new(0);

    pub fn set(milliseconds: u64) {
        PAUSE_MS.store(milliseconds, Ordering::Relaxed);
    }

    pub(super) fn wait() {
        std::thread::sleep(std::time::Duration::from_millis(
            PAUSE_MS.load(Ordering::Relaxed),
        ));
    }
}

/// Failed starts of a watcher, counted across folders: a folder is retried every few seconds,
/// and an exhausted inotify limit fails every one of them, so only the first is logged.
static UNAVAILABLE: Mutex<Streak> = Mutex::new(Streak::once());

/// No watcher could start at `stage`. The caller polls instead; the notify error carries paths
/// and text, so only the stage is logged.
fn unavailable<T>(stage: &str) -> Option<T> {
    report_unavailable(&UNAVAILABLE, stage);
    None
}

/// Logs the failed start unless the streak says it repeats one already logged.
fn report_unavailable(streak: &Mutex<Streak>, stage: &str) -> bool {
    let first = first_failure(streak, stage);
    if first {
        soyeht_platform::log::event("engine.file_watch_unavailable", &[("stage", stage)]);
    }
    first
}

/// The file events of one folder tree. Dropping it stops the stream.
pub(crate) struct EventStream {
    pub(crate) receiver: Receiver<notify::Result<Event>>,
    _watcher: Option<RecommendedWatcher>,
}

impl EventStream {
    /// Follows `root` and everything below it; `None` where no watcher can start.
    pub(crate) fn follow(root: &Path) -> Option<Self> {
        #[cfg(feature = "test-hooks")]
        slow_start::wait();
        let (sender, receiver) = mpsc::channel();
        let Ok(mut watcher) = notify::recommended_watcher(sender) else {
            return unavailable("create");
        };
        if watcher.watch(root, RecursiveMode::Recursive).is_err() {
            return unavailable("watch");
        }
        recovered(&UNAVAILABLE);
        Some(Self {
            receiver,
            _watcher: Some(watcher),
        })
    }

    /// A stream the test feeds, so event handling runs without the operating system.
    #[cfg(test)]
    pub(crate) fn fed(receiver: Receiver<notify::Result<Event>>) -> Self {
        Self {
            receiver,
            _watcher: None,
        }
    }
}

/// When the events seen so far are ready to be read.
#[derive(Default)]
pub(crate) struct Burst {
    started_ms: Option<i64>,
    last_event_ms: i64,
}

impl Burst {
    /// Records that an event arrived at `now`.
    pub(crate) fn note(&mut self, now: i64) {
        self.started_ms.get_or_insert(now);
        self.last_event_ms = now;
    }

    /// True once events stopped for `QUIET_MS` or the burst ran `BURST_LIMIT_MS`.
    pub(crate) fn settled(&self, now: i64) -> bool {
        self.started_ms.is_some_and(|started| {
            now - self.last_event_ms >= QUIET_MS || now - started >= BURST_LIMIT_MS
        })
    }

    /// Starts the next burst from nothing.
    pub(crate) fn end(&mut self) {
        self.started_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_that_keeps_failing_is_logged_once_until_it_recovers() {
        let streak = Mutex::new(Streak::once());
        let logged: Vec<bool> = (0..5)
            .map(|_| report_unavailable(&streak, "watch"))
            .collect();
        assert_eq!(logged, [true, false, false, false, false]);
        assert!(
            report_unavailable(&streak, "create"),
            "another stage is another failure"
        );
        recovered(&streak);
        assert!(report_unavailable(&streak, "create"));
    }
}
