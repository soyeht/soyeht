//! The notice "Soyeht closed unexpectedly" (ADR 0127, contract 16): looked for once when the
//! window opens, gone when the person dismisses it or copies the report.

use super::Workbench;
use gpui::{AppContext as _, Context};
use soyeht_client::crash_notice;
use soyeht_platform::log;

impl Workbench {
    /// Asks the `soyeht` program whether a crash report waits for the person, and shows the
    /// notice if one does. A failure only means no notice; it is logged by its kind.
    pub(crate) fn check_crash_reports(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { crash_notice::pending() }).await;
            // Once the window is gone nobody waits for the answer.
            this.update(cx, |this, cx| {
                match result {
                    Ok(shown) => this.crash_notice = shown,
                    Err(failure) => log::failure("desktop.crash_notice_failed", &failure),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Hides the notice, and tells the program the reports it showed were seen so it does not
    /// come back for them. Does nothing while the notice is not on screen.
    pub(crate) fn acknowledge_crash_notice(&mut self, cx: &mut Context<Self>) {
        let shown = std::mem::take(&mut self.crash_notice);
        if shown.is_empty() {
            return;
        }
        cx.notify();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_spawn(async move { crash_notice::acknowledge(&shown) })
                .await;
            if let Err(failure) = result {
                log::failure("desktop.crash_notice_failed", &failure);
            }
        })
        .detach();
    }
}
