//! The notice "Soyeht closed unexpectedly" (ADR 0127, contract 16): one quiet row under the top
//! bar with "Copy report", which is Copy diagnostics (the report is part of it), and "Dismiss".

use super::{preferences::pill_button, prelude::*};
use crate::theme::{Palette, pane_layout, space, type_scale};
use soyeht_protocol::client_policy::{
    CRASH_NOTICE_COPY, CRASH_NOTICE_DISMISS, CRASH_NOTICE_SUMMARY, CRASH_NOTICE_TITLE,
};

impl Workbench {
    pub(super) fn crash_notice_row(
        &self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        if !self.crash_notice {
            return None;
        }
        let copy =
            self.diagnostics_button(palette, "crash-notice-copy".into(), CRASH_NOTICE_COPY, cx);
        let dismiss = pill_button(
            palette,
            "crash-notice-dismiss".into(),
            None,
            CRASH_NOTICE_DISMISS,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.acknowledge_crash_notice(cx)));
        Some(
            row()
                .id("crash-notice")
                .flex_none()
                .items_center()
                .gap(px(space::X10))
                .px(px(pane_layout::SCREEN_PADDING + pane_layout::GRID_PADDING))
                .py(px(space::X8))
                .child(
                    chrome_text(FontWeight::BOLD, type_scale::CONTROL)
                        .text_color(rgb(palette.ink))
                        .child(CRASH_NOTICE_TITLE),
                )
                .child(
                    div().flex_1().min_w_0().truncate().child(
                        chrome_text(FontWeight::NORMAL, type_scale::CAPTION)
                            .text_color(rgb(palette.secondary))
                            .child(CRASH_NOTICE_SUMMARY),
                    ),
                )
                .children(self.diagnostics_message(palette))
                .child(copy)
                .child(dismiss),
        )
    }
}
