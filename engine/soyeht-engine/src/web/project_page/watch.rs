//! The watch of one project page: a change to a file the page's server gave since the page last
//! loaded counts a reload and raises the tick, which tells the pane's host (`reload_project_pages`).
//! A change to any other file under the folder (a build output, `node_modules`) reloads nothing,
//! and a page that keeps changing is reloaded at a growing interval.
//!
//! Starting the operating system's event stream can take seconds on a busy machine, so it
//! happens on a thread of its own, which then reads the events with a blocking wait: it wakes for
//! an event, or for the end of a pending burst, and never otherwise. It ends when the page's hold
//! on the stream is dropped, which disconnects the events. A system with no event stream reloads
//! nothing: the person reloads.

use super::Served;
use crate::file_events::{Burst, EventStream, Hold};
use notify::{Event, EventKind};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// The shortest time between two reloads of a page that has just been reloaded.
const BASE_GAP_MS: i64 = 500;
/// The longest the gap grows to while changes keep coming: a save after a build or a burst of
/// edits is on screen within this, and no page is reloaded more than about once in it.
const MAX_GAP_MS: i64 = 2_000;

/// When a burst of changes becomes a reload: once it has settled (`Burst`), and no sooner than the
/// gap after the last reload. The gap doubles, up to [`MAX_GAP_MS`], while a reload follows the one
/// before it within two gaps, and is back at its start when a change arrives after a quiet gap.
#[derive(Default)]
pub(super) struct Pacer {
    burst: Burst,
    gap_ms: i64,
    last_ms: Option<i64>,
}

impl Pacer {
    pub(super) fn note(&mut self, now: i64) {
        self.burst.note(now);
    }

    /// The gap in force: the start's, until a reload has made it grow.
    fn gap(&self) -> i64 {
        self.gap_ms.max(BASE_GAP_MS)
    }

    /// When the pending burst is to be reloaded, if there is one.
    pub(super) fn due_ms(&self) -> Option<i64> {
        self.burst
            .due_ms()
            .map(|due| self.last_ms.map_or(due, |last| due.max(last + self.gap())))
    }

    /// Whether the page is to be reloaded now; if it is, the burst is spent.
    pub(super) fn fire(&mut self, now: i64) -> bool {
        if !self.due_ms().is_some_and(|due| now >= due) {
            return false;
        }
        self.burst.end();
        let gap = self.gap();
        self.gap_ms = match self.last_ms {
            Some(last) if now - last < 2 * gap => (gap * 2).min(MAX_GAP_MS),
            _ => BASE_GAP_MS,
        };
        self.last_ms = Some(now);
        true
    }
}

/// Follows `root`: each reload the pacer allows adds one to `changes` and raises `kick`. The
/// returned hold is the page's; the thread ends when it is dropped.
pub(super) fn follow(
    root: PathBuf,
    served: Arc<Served>,
    kick: Arc<Notify>,
    changes: Arc<AtomicU64>,
) -> Arc<Mutex<Option<Hold>>> {
    let hold = Arc::new(Mutex::new(None));
    let page = Arc::downgrade(&hold);
    let started = std::thread::Builder::new()
        .name("project-page-watch".into())
        .spawn(move || {
            // The stream's own signal is every event; the tick is raised once per reload.
            let Some(stream) = EventStream::follow(&root, Arc::new(Notify::new())) else {
                return;
            };
            let (events, held) = stream.split();
            // The page may have ended while the stream started.
            let Some(page) = page.upgrade() else {
                return;
            };
            *page.lock().unwrap_or_else(PoisonError::into_inner) = Some(held);
            drop(page);
            run(&events, &served, &changes, || kick.notify_one());
        });
    if started.is_err() {
        // The thread's error carries no more than the system's reason; the page is served and
        // only its reload on change is missing.
        soyeht_platform::log::event(
            "engine.project_page_watch_unavailable",
            &[("stage", "thread")],
        );
    }
    hold
}

/// Reads the events until they end, and calls `reload` for each reload the pacer allows.
pub(super) fn run(
    events: &Receiver<notify::Result<Event>>,
    served: &Served,
    changes: &AtomicU64,
    reload: impl Fn(),
) {
    let started = Instant::now();
    let now_ms = || i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let mut pacer = Pacer::default();
    loop {
        let event = match pacer.due_ms() {
            Some(due) => events.recv_timeout(Duration::from_millis(
                u64::try_from(due - now_ms()).unwrap_or(0),
            )),
            None => events.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match event {
            // An error of the stream (it lost events) may hide a change: reload to be safe.
            Ok(event) if event.as_ref().map_or(true, |event| relevant(event, served)) => {
                pacer.note(now_ms());
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if pacer.fire(now_ms()) {
            changes.fetch_add(1, Ordering::Relaxed);
            reload();
        }
    }
}

/// Whether an event is a change to a file the page was given: not a read (serving a file is one,
/// and would reload the page for ever) and not any other file under the folder.
fn relevant(event: &Event, served: &Served) -> bool {
    !matches!(event.kind, EventKind::Access(_)) && served.any_of(&event.paths)
}
