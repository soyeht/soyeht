//! The update check of the window (ADR 0122): once at start and then each day while it is on,
//! and whenever the person asks. The work is `soyeht_client::update`; this runs it off the
//! window's thread and keeps the person's choice among the device's own.

use crate::workbench::Workbench;
use gpui::{AppContext as _, Context};
use soyeht_client::{local_preferences::LocalPreferences, update};
use soyeht_protocol::update_policy::CHECK_INTERVAL_SECONDS;
use std::time::Duration;

impl Workbench {
    /// Reads the person's choice and starts the daily check. Called once, when the window opens.
    pub(crate) fn start_update_checks(&mut self, cx: &mut Context<Self>) {
        let enabled = self
            .preferences
            .as_deref()
            .is_none_or(|directory| LocalPreferences::load(directory).checks_for_updates());
        self.updates.set_enabled(enabled);
        self.update_loop = Some(cx.spawn(async move |this, cx| {
            loop {
                if this
                    .update(cx, |this, cx| this.run_update_check(cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(CHECK_INTERVAL_SECONDS))
                    .await;
            }
        }));
    }

    /// Asks now, unless the check is off or one is already running.
    pub(crate) fn run_update_check(&mut self, cx: &mut Context<Self>) {
        if !self.updates.begin() {
            return;
        }
        cx.notify();
        let work = cx.background_spawn(async { update::check() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            // Once the window is gone the update fails, and nobody waits for the answer.
            this.update(cx, |this, cx| {
                this.updates.finish(result);
                cx.notify();
            })
        })
        .detach();
    }

    /// The switch of Preferences: applies the choice now and keeps it for the next launch.
    pub(crate) fn set_update_checks(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.updates.set_enabled(enabled);
        if let Some(directory) = self.preferences.as_deref()
            && let Err(error) = LocalPreferences::change(directory, |saved| {
                saved.check_for_updates = Some(enabled);
            })
        {
            soyeht_platform::log::failure("desktop.update_choice_not_saved", &error);
        }
        self.run_update_check(cx);
        cx.notify();
    }
}
