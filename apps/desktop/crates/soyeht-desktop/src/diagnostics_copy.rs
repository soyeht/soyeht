//! "Copy diagnostics" of the window (ADR 0118, contract 14 section 4): the report of `soyeht
//! diagnostics` on the clipboard. The work is `soyeht_client::diagnostics`; this runs it off the
//! window's thread, writes the clipboard and records how it ended.

use crate::workbench::Workbench;
use gpui::{AppContext as _, ClipboardItem, Context};
use soyeht_client::diagnostics;
use soyeht_platform::log;

impl Workbench {
    /// Collects the report and puts it on the clipboard, unless a collection is already running.
    /// The person is told how it ended either way.
    pub(crate) fn copy_diagnostics(&mut self, cx: &mut Context<Self>) {
        if !self.diagnostics.begin() {
            return;
        }
        cx.notify();
        let work = cx.background_spawn(async move { diagnostics::collect() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            // Once the window is gone nobody waits for the answer.
            this.update(cx, |this, cx| {
                let result = result.map(|report| {
                    cx.write_to_clipboard(ClipboardItem::new_string(report));
                });
                if let Err(failure) = &result {
                    log::failure("desktop.diagnostics_failed", failure);
                }
                this.diagnostics.finish(result);
                cx.notify();
            })
        })
        .detach();
    }
}
