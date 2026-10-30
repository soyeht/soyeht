//! The "Copy diagnostics" action (ADR 0118, contract 14 section 4): a button that puts the
//! redacted report on the clipboard, and the sentence that says how that went. The Setup page
//! shows it with its explanation, and the stage shows the button alone while the engine is not
//! running, which is when a person needs the report most. The Mac shows the same sentences.

use super::{preferences::pill_button, prelude::*};
use crate::theme::{Palette, pane_interior, space, type_scale};
use soyeht_protocol::client_policy::{DIAGNOSTICS_ACTION, DIAGNOSTICS_SUMMARY, DIAGNOSTICS_TITLE};

impl Workbench {
    /// The block of the Setup page: title, what the report is, and the action.
    pub(super) fn diagnostics_section(
        &self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        column()
            .gap(px(space::X8))
            .child(chrome_text(FontWeight::BOLD, type_scale::SUBHEADING).child(DIAGNOSTICS_TITLE))
            .child(
                chrome_text(FontWeight::NORMAL, type_scale::CAPTION)
                    .text_color(rgb(palette.secondary))
                    .child(DIAGNOSTICS_SUMMARY),
            )
            .child(self.diagnostics_action(palette, "setup", cx))
    }

    /// The button and, after it, the sentence about the last try. `place` keeps the ids of two
    /// copies of the action that are on screen together apart.
    pub(super) fn diagnostics_action(
        &self,
        palette: &'static Palette,
        place: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        // One collection at a time: the button waits for the one that runs.
        let working = self.diagnostics.working();
        let copy = pill_button(
            palette,
            SharedString::from(format!("diagnostics-copy-{place}")),
            Some(Icon::Copy),
            DIAGNOSTICS_ACTION,
        )
        .when(working, |button| {
            button
                .opacity(pane_interior::DISABLED_OPACITY)
                .cursor_default()
        })
        .when(!working, |button| {
            button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_diagnostics(cx)))
        });
        let color = if self.diagnostics.is_failure() {
            palette.warning()
        } else {
            palette.secondary
        };
        row()
            .gap(px(space::X10))
            .child(copy)
            .children(self.diagnostics.message().map(|message| {
                div().flex_1().min_w_0().child(
                    chrome_text(FontWeight::NORMAL, type_scale::CAPTION)
                        .text_color(rgb(color))
                        .child(message),
                )
            }))
    }
}
