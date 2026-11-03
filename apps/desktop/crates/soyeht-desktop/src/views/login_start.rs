//! Starting Soyeht at login (ADR 0136, contract 13 section 3): the question the window asks once,
//! as the Mac asks it, and the setting in the Setup panel that stays.

use super::{preferences::pill_button, switch_popover::well_row, ui};
use crate::{
    theme::{Palette, pane_interior, space, type_scale},
    workbench::Workbench,
};
use gpui::{self, AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, prelude::*};
use gpui::{div, px, rgb};
use soyeht_client::login_start::text;

impl Workbench {
    /// The question under the top bar, until the person answers it: "Turn on" registers the
    /// services, "Not now" is remembered. Nothing is registered unasked, since a system lists
    /// every background program (contract 13, section 3).
    pub(super) fn login_start_question(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.login_start.should_ask() {
            return None;
        }
        let palette = self.theme.palette();
        let turn_on = pill_button(palette, "login-start-turn-on".into(), None, text::TURN_ON)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_login_start(true, cx)));
        let not_now = pill_button(palette, "login-start-not-now".into(), None, text::NOT_NOW)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.decline_login_start(cx)));
        // The words wrap at a narrow window; the buttons keep their size.
        let words = ui::column()
            .flex_1()
            .min_w_0()
            .items_start()
            .gap(px(space::X2))
            .child(
                ui::chrome_text(FontWeight::SEMIBOLD, type_scale::CONTROL)
                    .text_color(rgb(palette.ink))
                    .child(text::QUESTION),
            )
            .child(caption(
                &format!("{} {}", text::SUMMARY, text::LATER),
                palette.secondary,
            ));
        Some(
            ui::notice_row()
                .id("login-start-question")
                .child(words)
                .child(turn_on)
                .child(not_now)
                .into_any_element(),
        )
    }

    /// The Start at login block of the Setup panel: the switch, and what it does.
    pub(super) fn login_start_section(
        &self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = &self.login_start;
        let (on, can_change) = (state.is_on(), state.can_change());
        let mut toggle = well_row(palette)
            .py(px(space::X6))
            .id(ui::id("login-start", "toggle"))
            .gap(px(space::X10))
            .child(div().flex_1().min_w_0().child(text::SETTING))
            .child(ui::switch(on, palette));
        if can_change {
            toggle = toggle.cursor_pointer().on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.set_login_start(!on, cx)),
            );
        } else {
            toggle = toggle.opacity(pane_interior::DISABLED_OPACITY);
        }
        let warning = |failure: bool| {
            if failure {
                palette.warning()
            } else {
                palette.secondary
            }
        };
        ui::section(palette, text::TITLE, text::SUMMARY)
            .child(toggle)
            .children(
                (!state.is_available()).then(|| caption(text::UNAVAILABLE, palette.secondary)),
            )
            .children(
                (on && can_change).then(|| caption(text::TURN_OFF_WARNING, palette.secondary)),
            )
            .children(
                state
                    .message()
                    .map(|(sentence, failure)| caption(sentence, warning(failure))),
            )
    }
}

/// A sentence under a heading, in `color`.
fn caption(sentence: &str, color: u32) -> Div {
    ui::chrome_text(FontWeight::NORMAL, type_scale::CAPTION)
        .text_color(rgb(color))
        .child(SharedString::from(sentence.to_owned()))
}
