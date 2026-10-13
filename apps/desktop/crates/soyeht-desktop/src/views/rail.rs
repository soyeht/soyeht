//! The rail down the left of the Mac window: the places, then the usage rings with their
//! percentages, then settings at the bottom.

use super::{
    ui::{column, hint, icon, id, waiting_badge, working_dot},
    usage_ring,
};
use crate::{
    assets::Icon,
    destination::Destination,
    open_menu::Menu,
    theme::{Palette, desktop, layout, pane_layout, radius, space},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement, IntoElement, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
    rgb,
};

impl Workbench {
    pub(super) fn rail(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let places: Vec<AnyElement> = Destination::ALL
            .into_iter()
            .map(|place| self.rail_button(place, palette, cx))
            .collect();
        // A ring opens the usage of its provider, as on the Mac.
        let rings: Vec<AnyElement> = self
            .state
            .usage()
            .to_vec()
            .into_iter()
            .map(|reading| {
                let name = reading.provider.display_name();
                let provider = reading.provider.clone();
                div()
                    .id(id("usage", name))
                    .cursor_pointer()
                    .tooltip(hint(format!("{name} usage"), palette))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_usage(provider.clone(), cx)
                    }))
                    .child(usage_ring::ring(&reading, palette))
                    .into_any_element()
            })
            .collect();
        column()
            .w(px(layout::RAIL_WIDTH))
            .flex_none()
            .h_full()
            .items_center()
            .gap(px(space::X6))
            // The rail is as tall as the panes beside it, and its buttons sit inside that.
            .pt(px(pane_layout::SCREEN_PADDING
                + pane_layout::GRID_PADDING
                + space::X12))
            .pb(px(pane_layout::SCREEN_PADDING
                + pane_layout::GRID_PADDING
                + space::X12))
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
        // The agents' button carries what needs the person: a badge with the requests waiting,
        // else a dot while an agent works.
        let agents = place == Destination::Agents;
        let waiting = if agents {
            self.state.waiting_requests(None)
        } else {
            0
        };
        let working = agents && self.state.has_working_agent(None);
        rail_slot(id("rail", place.label()), selected, palette)
            .relative()
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.select_destination(place, cx)),
            )
            .tooltip(hint(place.label(), palette))
            .child(icon(
                place.icon(),
                crate::theme::glyph::RAIL,
                if selected {
                    palette.accent
                } else {
                    palette.secondary
                },
            ))
            .when(waiting > 0, |button| {
                button.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .child(waiting_badge(waiting, palette)),
                )
            })
            .when(waiting == 0 && working, |button| {
                button.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .p(px(space::X2))
                        .child(working_dot(palette)),
                )
            })
            .into_any_element()
    }

    fn settings_button(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.menus.is_open(&Menu::Settings);
        rail_slot("rail-settings".into(), open, palette)
            .tooltip(hint("Settings", palette))
            .on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_menu(Menu::Settings, cx)),
            )
            .child(icon(
                Icon::Settings,
                crate::theme::glyph::RAIL,
                if open {
                    palette.accent
                } else {
                    palette.secondary
                },
            ))
            .into_any_element()
    }
}

/// A square button of the rail: on the well while it is the selected one, on the hover color
/// under the pointer otherwise.
fn rail_slot(key: SharedString, selected: bool, palette: &Palette) -> Stateful<Div> {
    div()
        .id(key)
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
}
