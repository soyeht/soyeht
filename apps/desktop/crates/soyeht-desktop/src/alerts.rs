//! System notifications for the requests agents wait on (ADR 0103).
//!
//! The window raises one when an agent starts waiting for a permission or an answer and the
//! window is not in front, as the Mac does; the rule is `soyeht_client::attention` and its cases
//! are `contracts/vectors/agent-attention.json`. A click on the notification brings the window
//! forward and opens that pane. The system calls block, so each notification has a thread that
//! shows it, waits for the click and reports back to the window.

use crate::{
    destination::Destination,
    os_notify::{self, Outcome, Toast},
    workbench::Workbench,
};
use gpui::{Context, Window};
use soyeht_client::{
    Connection,
    attention::{self, Cursor, Request},
};
use soyeht_platform::log::{self, Failure};
use std::{
    env::VarError,
    mem::replace,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// `off` or `0` turns notifications off for this run, for a disposable engine or a test.
const SWITCH: &str = "SOYEHT_NOTIFICATIONS";
/// Notifications that wait for a click at once, a thread each. A person who is away while agents
/// keep asking sees more than this in the system's own list.
const LISTENERS: usize = 16;

/// What the window needs to open when a notification is clicked.
pub(crate) struct Target {
    workspace_id: String,
    pane_id: String,
    identity: String,
}

/// What a notification's thread tells the window.
pub(crate) enum Report {
    Delivered,
    Failed(os_notify::Error),
    Activated(Target),
}

/// Whether the value of [`SWITCH`] leaves notifications on.
fn switch_allows(value: Result<&str, &VarError>) -> bool {
    !matches!(value, Ok("off" | "0"))
}

pub(crate) struct Alerts {
    enabled: bool,
    cursor: Cursor,
    /// The threads that wait for a click now.
    listeners: Arc<AtomicUsize>,
    reports: async_channel::Sender<Report>,
    inbox: Option<async_channel::Receiver<Report>>,
    /// The last request was skipped for want of a slot: logged once until one is taken again.
    saturated: bool,
    /// The kind of the failure last logged: a repeat of it is not logged again.
    failing: Option<String>,
}

/// One of the [`LISTENERS`]: held by a notification's thread, released when the thread ends or
/// unwinds, and when the thread could not be started (the closure that owns it is dropped).
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(listeners: &Arc<AtomicUsize>) -> Option<Self> {
        if listeners.fetch_add(1, Ordering::AcqRel) >= LISTENERS {
            listeners.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self(listeners.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Shows the notification and waits for what the person does with it.
fn show_and_wait(toast: &Toast, reports: &async_channel::Sender<Report>, target: Target) -> Report {
    match os_notify::show(toast) {
        Ok(shown) => {
            // The window may be gone by now; the reports then have no reader.
            if reports.send_blocking(Report::Delivered).is_ok()
                && shown.wait() == Outcome::Activated
            {
                Report::Activated(target)
            } else {
                Report::Delivered
            }
        }
        Err(error) => Report::Failed(error),
    }
}

impl Alerts {
    pub(crate) fn new() -> Self {
        let (reports, inbox) = async_channel::unbounded();
        Self {
            enabled: attention::ENABLED_BY_DEFAULT
                && switch_allows(std::env::var(SWITCH).as_deref()),
            cursor: Cursor::default(),
            listeners: Arc::default(),
            reports,
            inbox: Some(inbox),
            saturated: false,
            failing: None,
        }
    }

    /// Whether this skip is the first since a slot was last taken: the one that is logged.
    fn first_skip(&mut self) -> bool {
        !replace(&mut self.saturated, true)
    }

    /// Shows `request` on a thread of its own, which reports what became of it.
    fn post(&mut self, request: &Request, workspace_name: Option<&str>) {
        let Some(slot) = Slot::take(&self.listeners) else {
            // A notification skipped here is lost, not deferred: the cursor has seen it.
            if self.first_skip() {
                log::event(
                    "desktop.notification_skipped",
                    &[("reason", "too_many_open")],
                );
            }
            return;
        };
        self.saturated = false;
        let body = attention::body(&request.pane_name, workspace_name);
        let target = Target {
            workspace_id: request.workspace_id.clone(),
            pane_id: request.pane_id.clone(),
            identity: request.identity(),
        };
        let reports = self.reports.clone();
        let spawned = std::thread::Builder::new()
            .name("soyeht-notification".to_owned())
            .spawn(move || {
                let toast = Toast {
                    title: attention::TITLE,
                    body: &body,
                };
                // A panic of the system call (a bus that drops mid-wait) is a failure to show, and
                // the slot is released either way.
                let report =
                    catch_unwind(AssertUnwindSafe(|| show_and_wait(&toast, &reports, target)))
                        .unwrap_or(Report::Failed(os_notify::Error::Unavailable));
                drop(slot);
                if matches!(report, Report::Activated(_) | Report::Failed(_)) {
                    // Nothing is left to tell once the window is gone.
                    drop(reports.send_blocking(report));
                }
            });
        if let Err(error) = spawned {
            log::failure("desktop.notification_thread_failed", &error);
        }
    }

    /// Logs a failure when its kind differs from the last one, and the recovery after a failure.
    fn note(&mut self, report: &Report) {
        match report {
            Report::Failed(error) => {
                let kind = error.failure_kind();
                if self.failing.as_deref() != Some(kind.as_str()) {
                    log::failure("desktop.notification_failed", error);
                    self.failing = Some(kind);
                }
            }
            Report::Delivered if self.failing.take().is_some() => {
                log::event("desktop.notification_recovered", &[]);
            }
            _ => {}
        }
    }
}

impl Workbench {
    /// Raises a notification for each request that is new, when the window is not in front.
    pub(crate) fn raise_alerts(&mut self, cx: &mut Context<Self>) {
        let connected = self.state.connection == Connection::Connected;
        let new = self
            .alerts
            .cursor
            .new_requests(&self.state.attention(), connected);
        if new.is_empty()
            || !attention::should_post(self.alerts.enabled, cx.active_window().is_some())
        {
            return;
        }
        for request in &new {
            let workspace = self.state.workspace(&request.workspace_id);
            self.alerts
                .post(request, workspace.map(|workspace| workspace.name.as_str()));
        }
    }

    /// Starts reading what the notifications' threads report. The window calls it once, when it
    /// opens: a click needs the window to bring forward.
    pub(crate) fn listen_for_alerts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inbox) = self.alerts.inbox.take() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(report) = inbox.recv().await {
                let handled = this.update_in(cx, |this, window, cx| {
                    this.alerts.note(&report);
                    if let Report::Activated(target) = report {
                        this.open_alerted_pane(&target, window, cx);
                    }
                });
                if handled.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// A notification was clicked: brings the window forward and opens the pane, if its request
    /// is still waiting.
    fn open_alerted_pane(&mut self, target: &Target, window: &mut Window, cx: &mut Context<Self>) {
        let pending = attention::is_pending(
            &self.state.attention(),
            &target.workspace_id,
            &target.pane_id,
            &target.identity,
        );
        // A click on a notification whose request was answered meanwhile opens nothing. The line
        // tells a click that did nothing from one that never arrived.
        log::event(
            "desktop.notification_clicked",
            &[("pending", if pending { "true" } else { "false" })],
        );
        if !pending {
            return;
        }
        self.select_destination(Destination::Agents, cx);
        self.focus_pane(&target.pane_id, window, cx);
        window.activate_window();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os_notify::Error;

    #[test]
    fn only_off_or_zero_turns_them_off() {
        assert!(switch_allows(Err(&VarError::NotPresent)));
        assert!(switch_allows(Ok("on")));
        assert!(!switch_allows(Ok("off")));
        assert!(!switch_allows(Ok("0")));
    }

    #[test]
    fn a_repeated_failure_is_remembered_once_and_a_delivery_ends_it() {
        let mut alerts = Alerts::new();
        alerts.note(&Report::Failed(Error::Unavailable));
        assert_eq!(alerts.failing.as_deref(), Some("unavailable"));
        alerts.note(&Report::Failed(Error::Unavailable));
        assert_eq!(alerts.failing.as_deref(), Some("unavailable"));
        alerts.note(&Report::Failed(Error::Unsupported));
        assert_eq!(alerts.failing.as_deref(), Some("unsupported"));
        alerts.note(&Report::Delivered);
        assert_eq!(alerts.failing, None);
    }

    fn request() -> Request {
        Request {
            request_id: "r".into(),
            pane_id: "p".into(),
            workspace_id: "w".into(),
            pane_name: "Reviewer".into(),
            generation: 1,
        }
    }

    #[test]
    fn no_more_than_the_limit_wait_for_a_click_at_once() {
        let mut alerts = Alerts::new();
        alerts.listeners.store(LISTENERS, Ordering::Release);
        alerts.post(&request(), Some("soyeht"));
        assert_eq!(alerts.listeners.load(Ordering::Acquire), LISTENERS);
        assert!(alerts.saturated);
    }

    #[test]
    fn a_slot_is_released_when_its_holder_ends_or_panics() {
        let listeners = Arc::new(AtomicUsize::new(0));
        let slot = Slot::take(&listeners).unwrap();
        assert_eq!(listeners.load(Ordering::Acquire), 1);
        drop(slot);
        assert_eq!(listeners.load(Ordering::Acquire), 0);
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            let _slot = Slot::take(&listeners).unwrap();
            panic!("the system call panics");
        }));
        assert!(panicked.is_err());
        assert_eq!(listeners.load(Ordering::Acquire), 0);
        let held: Vec<Slot> = (0..LISTENERS)
            .filter_map(|_| Slot::take(&listeners))
            .collect();
        assert_eq!(held.len(), LISTENERS);
        assert!(Slot::take(&listeners).is_none());
        assert_eq!(listeners.load(Ordering::Acquire), LISTENERS);
    }

    /// A system without notifications reports at once, so the thread's whole life can be seen.
    #[cfg(not(any(target_os = "linux", windows)))]
    #[test]
    fn the_slot_is_free_again_after_a_thread_reports_a_failure() {
        let mut alerts = Alerts::new();
        let inbox = alerts.inbox.take().unwrap();
        alerts.post(&request(), Some("soyeht"));
        let report = inbox.recv_blocking().unwrap();
        assert!(matches!(
            report,
            Report::Failed(os_notify::Error::Unsupported)
        ));
        assert_eq!(alerts.listeners.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_skip_is_logged_once_until_a_slot_is_taken_again() {
        let mut alerts = Alerts::new();
        assert!(alerts.first_skip());
        assert!(!alerts.first_skip());
        assert!(!alerts.first_skip());
    }
}
