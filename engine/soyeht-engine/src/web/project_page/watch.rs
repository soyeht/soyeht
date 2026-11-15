//! The watch of one project page: a change under the page's directory raises a counter and the
//! tick, once a burst of changes has settled, and the tick tells the pane's host to reload.
//!
//! Starting the operating system's event stream can take seconds on a busy machine, so it
//! happens on a thread of its own, which also settles the bursts, and neither a request nor the
//! Core lock waits for it. A system with no event stream reloads nothing: the person reloads.

use crate::file_events::{Burst, EventStream, QUIET_MS};
use notify::{Event, EventKind};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::RecvTimeoutError,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// How often a thread with nothing to settle looks at its stop flag.
const IDLE: Duration = Duration::from_millis(250);

/// Follows `root` until `stop` is set: each settled burst of changes adds one to `changes` and
/// raises `kick`.
pub(super) fn follow(
    root: PathBuf,
    kick: Arc<Notify>,
    changes: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) {
    let started = std::thread::Builder::new()
        .name("project-page-watch".into())
        .spawn(move || {
            // The stream's own signal is every event; the tick is raised once per burst.
            let events = EventStream::follow(&root, Arc::new(Notify::new()));
            if let Some(events) = events {
                settle(&events.receiver, &root, &changes, &stop, || {
                    kick.notify_one()
                });
            }
        });
    if started.is_err() {
        // The thread's error carries no more than the system's reason; the page is served and
        // only its reload on change is missing.
        soyeht_platform::log::event(
            "engine.project_page_watch_unavailable",
            &[("stage", "thread")],
        );
    }
}

/// Reads the stream until `stop` or its end, and calls `changed` once per settled burst.
pub(super) fn settle(
    receiver: &std::sync::mpsc::Receiver<notify::Result<Event>>,
    root: &Path,
    changes: &AtomicU64,
    stop: &AtomicBool,
    changed: impl Fn(),
) {
    let started = Instant::now();
    let now_ms = || i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let mut burst = Burst::default();
    while !stop.load(Ordering::Relaxed) {
        let wait = if burst.due_ms().is_some() {
            Duration::from_millis(QUIET_MS.unsigned_abs())
        } else {
            IDLE
        };
        match receiver.recv_timeout(wait) {
            // An error of the stream (it lost events) may hide a change: reload to be safe.
            Ok(event) if event.as_ref().map_or(true, |event| relevant(event, root)) => {
                burst.note(now_ms());
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if burst.settled(now_ms()) {
            burst.end();
            changes.fetch_add(1, Ordering::Relaxed);
            changed();
        }
    }
}

/// Whether an event is a change of a file the page can read: not a read (serving the file is
/// one, and would reload the page for ever) and not under a hidden part.
fn relevant(event: &Event, root: &Path) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && event.paths.iter().any(|path| {
            !path
                .strip_prefix(root)
                .is_ok_and(|relative| relative.components().any(super::hidden))
        })
}
