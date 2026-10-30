//! The Updates block of the Setup panel (ADR 0122): whether Soyeht asks for a newer version, what
//! it found and the buttons that open the package and the release notes in the browser. The Mac's
//! General panel shows the same sentences (`contracts/vectors/update-status.json`).

use super::{
    preferences::pill_button,
    switch_popover::well_row,
    ui::{chrome_text, column, id, row, switch},
};
use crate::{
    assets::Icon,
    theme::{Palette, space, type_scale},
    workbench::Workbench,
};
use gpui::{ClickEvent, Context, Div, FontWeight, SharedString, div, prelude::*, px, rgb};
use soyeht_protocol::update_policy::text;

impl Workbench {
    pub(super) fn updates_section(&self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let enabled = self.updates.enabled();
        let caption = |sentence: String, color: u32| {
            chrome_text(FontWeight::NORMAL, type_scale::CAPTION)
                .text_color(rgb(color))
                .child(SharedString::from(sentence))
        };
        let headline = caption(
            self.updates.headline(env!("CARGO_PKG_VERSION")),
            palette.ink,
        );
        let check_now = pill_button(
            palette,
            "updates-check-now".into(),
            Some(Icon::Refresh),
            text::CHECK_NOW,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.run_update_check(cx)));
        let offer = self.updates.offer().map(|offer| {
            let (package, notes) = (offer.package.url.clone(), offer.notes_url.clone());
            let download = pill_button(
                palette,
                "updates-download".into(),
                None,
                &self.updates.download_label().unwrap_or_default(),
            )
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&package)));
            let release_notes = pill_button(palette, "updates-notes".into(), None, text::NOTES)
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&notes)));
            row()
                .gap(px(space::X8))
                .child(download)
                .child(release_notes)
        });
        column()
            .gap(px(space::X8))
            .child(chrome_text(FontWeight::BOLD, type_scale::SUBHEADING).child(text::TITLE))
            .child(caption(text::SUMMARY.to_owned(), palette.secondary))
            .child(
                well_row(palette)
                    .py(px(space::X6))
                    .id(id("updates", "toggle"))
                    .gap(px(space::X10))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set_update_checks(!enabled, cx)
                    }))
                    .child(div().flex_1().min_w_0().child(text::TOGGLE))
                    .child(switch(enabled, palette)),
            )
            .child(
                row()
                    .gap(px(space::X10))
                    .child(div().flex_1().min_w_0().child(headline))
                    .when(enabled, |line| line.child(check_now)),
            )
            .children(
                self.updates
                    .warning()
                    .map(|warning| caption(warning.to_owned(), palette.warning())),
            )
            .children(offer)
    }
}
