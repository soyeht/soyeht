//! The Appearance panel of Preferences, as the Mac's: a card for each of the nine themes, with a
//! swatch of the theme's own surfaces, the name and a check.

use super::{
    preferences::page,
    ui::{Typeface, column, icon, id, row},
};
use crate::{
    assets::Icon,
    neo,
    theme::{Palette, ThemeName, desktop, elevation, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, div, px, rgb,
};

impl Workbench {
    /// The Appearance panel: the themes in two columns.
    pub(super) fn appearance_page(
        &mut self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        // The cards wrap: two to a line while the page is wide, one in a narrow window.
        let cards: Vec<AnyElement> = ThemeName::ALL
            .iter()
            .map(|&theme| theme_card(theme, theme == self.theme, palette, cx))
            .collect();
        page(
            palette,
            "Appearance",
            Some("Choose one neomorphic theme for every Soyeht window and pane."),
            None,
            column()
                .gap(px(space::X12))
                .child(row().flex_wrap().gap(px(space::X12)).children(cards))
                .child(
                    div()
                        .chrome()
                        .text_size(px(type_scale::SMALL))
                        .text_color(rgb(palette.muted))
                        .child("Your choice applies immediately."),
                ),
        )
    }
}

/// One theme: the swatch, its name and whether it is the chosen one.
fn theme_card(
    theme: ThemeName,
    selected: bool,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let own = theme.palette();
    row()
        .id(id("theme", theme.display_name()))
        .w(px(desktop::THEME_CARD_WIDTH))
        .min_h(px(desktop::THEME_CARD_HEIGHT))
        .flex_none()
        .gap(px(space::X12))
        .p(px(space::X12))
        .rounded(px(radius::CONTROL))
        .bg(rgb(if selected { palette.well } else { palette.face }))
        .border(px(if selected {
            desktop::THEME_CARD_SELECTED_BORDER
        } else {
            desktop::HAIRLINE
        }))
        .border_color(rgb(if selected {
            palette.accent
        } else {
            palette.well
        }))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.select_theme(theme, cx);
        }))
        .child(swatch(own))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .chrome()
                .text_size(px(type_scale::CAPTION))
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .child(theme.display_name()),
        )
        .child(if selected {
            div()
                .size(px(desktop::THEME_CHECK_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(palette.accent))
                .child(icon(Icon::Check, desktop::CHECK_GLYPH, palette.on_accent))
                .into_any_element()
        } else {
            icon(Icon::Circle, desktop::THEME_CHECK_SIZE, palette.muted).into_any_element()
        })
        .into_any_element()
}

/// A theme's own surfaces on its own canvas: a raised plate with the accent, and a well.
fn swatch(own: &'static Palette) -> Div {
    div()
        .relative()
        .w(px(desktop::SWATCH_WIDTH))
        .h(px(desktop::SWATCH_HEIGHT))
        .flex_none()
        .rounded(px(radius::CONTROL))
        .bg(rgb(own.canvas))
        .overflow_hidden()
        .child(
            neo::raised(own, radius::COMPACT, elevation::TOGGLE)
                .absolute()
                .left(px(desktop::SWATCH_PLATE_LEFT))
                .top(px(
                    (desktop::SWATCH_HEIGHT - desktop::SWATCH_PLATE_HEIGHT) / 2.0
                ))
                .w(px(desktop::SWATCH_PLATE_WIDTH))
                .h(px(desktop::SWATCH_PLATE_HEIGHT))
                .child(
                    div()
                        .absolute()
                        .left(px(space::X8))
                        .top(px((desktop::SWATCH_PLATE_HEIGHT
                            - desktop::SWATCH_ACCENT_HEIGHT)
                            / 2.0))
                        .w(px(space::X6))
                        .h(px(desktop::SWATCH_ACCENT_HEIGHT))
                        .rounded_full()
                        .bg(rgb(own.accent)),
                ),
        )
        .child(
            neo::well(own, radius::PILL, desktop::WELL_DEPTH)
                .absolute()
                .left(px(desktop::SWATCH_WELL_LEFT))
                .top(px(
                    (desktop::SWATCH_HEIGHT - desktop::SWATCH_WELL_SIZE) / 2.0
                ))
                .size(px(desktop::SWATCH_WELL_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(space::X5)).rounded_full().bg(rgb(own.ink))),
        )
}
