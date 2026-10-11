//! The rail down the left of the Mac window: the places, then the usage rings with their
//! percentages, then settings at the bottom.

use super::{
    menus,
    ui::{column, icon, id},
    usage_ring,
};
use crate::{
    assets::Icon,
    destination::Destination,
    open_menu::Menu,
    theme::{Palette, ThemeName, desktop, layout, radius, space},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, rgb,
};

impl Workbench {
    pub(super) fn rail(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let places: Vec<AnyElement> = Destination::ALL
            .into_iter()
            .map(|place| self.rail_button(place, palette, cx))
            .collect();
        let rings: Vec<AnyElement> = self
            .state
            .usage()
            .iter()
            .map(|reading| usage_ring::ring(reading, palette))
            .collect();
        column()
            .w(px(layout::RAIL_WIDTH))
            .flex_none()
            .h_full()
            .items_center()
            .gap(px(space::X6))
            .py(px(space::X12))
            .px(px(space::X8))
            .child(
                column()
                    .id("rail-places")
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .gap(px(space::X6))
                    .overflow_y_scroll()
                    .children(places)
                    .child(
                        div()
                            .w(px(layout::RAIL_DIVIDER_WIDTH))
                            .h(px(desktop::HAIRLINE))
                            .flex_none()
                            .my(px(space::X2))
                            .bg(rgb(palette.shadow_dark)),
                    )
                    .children(rings),
            )
            .child(self.settings_button(palette, cx))
    }

    fn rail_button(
        &mut self,
        place: Destination,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.destination == place;
        div()
            .id(id("rail", place.label()))
            .size(px(layout::RAIL_BUTTON_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::COMPACT))
            .cursor_pointer()
            .when(selected, |button| button.bg(rgb(palette.well)))
            .when(!selected, |button| {
                button.hover(|style| style.bg(rgb(palette.hover)))
            })
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.select_destination(place, cx)),
            )
            .child(icon(
                place.icon(),
                crate::theme::glyph::RAIL,
                if selected {
                    palette.accent
                } else {
                    palette.secondary
                },
            ))
            .into_any_element()
    }

    fn settings_button(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.menus.is_open(&Menu::Settings);
        div()
            .relative()
            .flex_none()
            .child(
                div()
                    .id("rail-settings")
                    .size(px(layout::RAIL_BUTTON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::COMPACT))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_menu(Menu::Settings, cx)
                    }))
                    .child(icon(
                        Icon::Settings,
                        crate::theme::glyph::RAIL,
                        palette.secondary,
                    )),
            )
            .when(open, |slot| {
                let body = self.settings_menu(palette, cx);
                slot.child(menus::popup(
                    palette,
                    Anchor::BottomLeft,
                    (
                        layout::RAIL_BUTTON_SIZE + space::X8,
                        layout::RAIL_BUTTON_SIZE,
                    ),
                    body,
                    cx,
                ))
            })
            .into_any_element()
    }

    /// Settings: the color theme, and how the engine connection stands.
    fn settings_menu(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let themes: Vec<AnyElement> = ThemeName::ALL
            .into_iter()
            .map(|theme| {
                menus::on_pick(
                    menus::item(
                        palette,
                        theme.display_name(),
                        theme.display_name(),
                        theme == self.theme,
                    ),
                    cx,
                    move |this, cx| {
                        this.theme = theme;
                        this.close_menu(cx);
                    },
                )
            })
            .collect();
        // The connection comes first: it is the short part, and a menu that scrolls keeps it in sight.
        menus::list(
            [
                menus::heading(palette, "Engine").into_any_element(),
                menus::note(
                    palette,
                    super::connection_notice(&self.state.connection).status,
                )
                .into_any_element(),
                menus::heading(palette, "Theme").into_any_element(),
            ]
            .into_iter()
            .chain(themes)
            .collect(),
        )
    }
}
