//! The parts the sheets of the window share (the new-task form, "Trust this folder?"): the raised
//! card, its title row with the way to close it, the round buttons of its footer, and the layer
//! that centers a panel over the whole window.

use super::{
    prelude::*,
    ui::{caption, chrome_text},
};
use crate::{
    neo,
    theme::{Palette, elevation, radius, space, type_scale},
};

/// The raised card of a sheet: its rows are spaced and padded as the Mac's sheets are, and it
/// takes the pointer so nothing under it does.
pub(super) fn card(palette: &'static Palette, key: &'static str) -> gpui::Stateful<Div> {
    neo::raised(palette, radius::CARD, elevation::PANEL)
        .id(key)
        .flex()
        .flex_col()
        .gap(px(space::X14))
        .px(px(space::X18 + space::X4))
        .py(px(space::X18))
        .occlude()
}

/// The title of a sheet with a line under it, and the close control on the right.
pub(super) fn header(
    palette: &'static Palette,
    title: &'static str,
    subtitle: impl Into<SharedString>,
    close: gpui::Stateful<Div>,
) -> AnyElement {
    row()
        .items_start()
        .gap(px(space::X10))
        .child(
            column()
                .flex_1()
                .min_w_0()
                .gap(px(space::X3))
                .child(chrome_text(FontWeight::BOLD, type_scale::SUBHEADING).child(title))
                .child(caption(palette).child(subtitle.into())),
        )
        .child(close)
        .into_any_element()
}

/// A round button of a sheet's footer: the accent one is the primary answer, the other is raised.
pub(super) fn button(
    palette: &'static Palette,
    key: SharedString,
    accent: bool,
) -> gpui::Stateful<Div> {
    let base = if accent {
        div()
            .bg(rgb(palette.accent))
            .text_color(rgb(palette.on_accent))
    } else {
        neo::raised(palette, radius::PILL, elevation::ACTION).text_color(rgb(palette.ink))
    };
    base.id(key)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(space::X6))
        .px(px(space::X14))
        .py(px(space::X6))
        .rounded_full()
        .chrome_bold(type_scale::CONTROL)
        .own_press()
}

/// A layer over the whole window that centers what it holds.
pub(super) fn centered_layer() -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
}
