//! Whether Soyeht starts when the person logs in (ADR 0136): the question the window asks once,
//! and the setting in Preferences. The work is `soyeht_client::login_start`; this runs it off
//! the window's thread and keeps the person's "no" among the device's own choices.

use super::Workbench;
use gpui::{AppContext as _, Context};
use soyeht_client::{
    diagnostics::Failure,
    local_preferences::LocalPreferences,
    login_start::{self, LoginStart},
};
use soyeht_platform::log;

impl Workbench {
    /// Reads the person's earlier "Not now" and asks the `soyeht` program whether the services are
    /// registered. Called once, when the window opens on an engine it may start itself.
    pub(crate) fn check_login_start(&mut self, cx: &mut Context<Self>) {
        let declined = self
            .preferences
            .as_deref()
            .map(LocalPreferences::load)
            .is_some_and(|saved| saved.declined_login_start());
        self.login_start = LoginStart::new(declined);
        if !self.login_start.begin_check() {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let answer = cx.background_spawn(async { login_start::status() }).await;
            // Once the window is gone nobody waits for the answer.
            this.update(cx, |this, cx| {
                if let Err(failure) = &answer
                    && *failure != Failure::Unavailable
                {
                    log::failure("desktop.login_start_status_failed", failure);
                }
                this.login_start.checked(answer);
                cx.notify();
            })
        })
        .detach();
    }

    /// The switch of Preferences, or "Turn on" of the question: registers or unregisters the
    /// services, and keeps the choice for the next launch.
    pub(crate) fn set_login_start(&mut self, on: bool, cx: &mut Context<Self>) {
        if !self.login_start.begin_change(on) {
            return;
        }
        cx.notify();
        // Turning it off ends the engine with its services: the window must not take that for
        // an uninstall by someone else, nor start an engine while it is under way.
        let change = self.engine.service_change();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let result = if on {
                        login_start::register()
                    } else {
                        login_start::unregister()
                    };
                    drop(change);
                    result
                })
                .await;
            this.update(cx, |this, cx| {
                if let Err(failure) = &result {
                    log::failure("desktop.login_start_change_failed", failure);
                }
                this.login_start.changed(on, result);
                this.keep_login_start_choice();
                cx.notify();
            })
        })
        .detach();
    }

    /// "Not now": the window does not ask again, and the setting stays in Preferences.
    pub(crate) fn decline_login_start(&mut self, cx: &mut Context<Self>) {
        self.login_start.decline();
        self.keep_login_start_choice();
        cx.notify();
    }

    /// Writes whether the person declined to the device's own choices, when it changed.
    fn keep_login_start_choice(&self) {
        let declined = self.login_start.declined();
        if let Some(directory) = self.preferences.as_deref()
            && LocalPreferences::load(directory).declined_login_start() != declined
            && let Err(error) = LocalPreferences::change(directory, |saved| {
                saved.login_start_declined = declined.then_some(true);
            })
        {
            log::failure("desktop.login_start_choice_not_saved", &error);
        }
    }
}
