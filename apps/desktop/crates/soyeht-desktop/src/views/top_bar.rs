//! The top bar of the Mac window: the rail toggle, the workspace tabs as rounded pills with a
//! close mark, a "+N" pill for the tabs that do not fit, the "+" that opens a pane, and on
//! Linux and Windows the window controls.

use super::{
    harness_menu::Pick,
    menus,
    ui::{OwnPress, icon, id, row, spacer},
    window_controls,
};
use crate::{
    assets::Icon,
    neo,
    open_menu::Menu,
    tab_plan,
    theme::{
        Palette, desktop, elevation, fonts, glyph, layout, pane_interior, radius, space, type_scale,
    },
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px, rgb,
};
use soyeht_client::Connection;
use std::rc::Rc;

/// Room kept for dragging the window by the bar.
const MINIMUM_DRAG: f32 = desktop::MINIMUM_DRAG_WIDTH;

impl Workbench {
    pub(super) fn top_bar(
        &mut self,
        palette: &'static Palette,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let gap = space::X10;
        let control = layout::TOP_BAR_CONTROL_HEIGHT;
        let on_mac = cfg!(target_os = "macos");
        let leading = if on_mac {
            layout::TOP_BAR_LEADING_INSET
        } else {
            space::X12
        };
        let shown = if on_mac {
            Vec::new()
        } else {
            window_controls::visible(window)
        };
        let controls = if shown.is_empty() {
            0.0
        } else {
            window_controls::width(shown.len()) + gap
        };
        let fixed = leading + 2.0 * (control + gap) + controls + space::X12 + MINIMUM_DRAG;
        let available = (width - fixed).max(0.0);

        let workspaces = self.state.workspaces();
        let names: Vec<&str> = workspaces.iter().map(|w| w.name.as_str()).collect();
        let selected = self
            .state
            .selected_workspace()
            .map(|w| w.workspace_id.clone());
        let active = workspaces
            .iter()
            .position(|w| Some(&w.workspace_id) == selected.as_ref());
        let plan = tab_plan::plan(&names, active, available);
        let tabs: Vec<AnyElement> = plan
            .visible
            .iter()
            .map(|&index| {
                let workspace = &workspaces[index];
                self.tab(
                    &workspace.workspace_id,
                    &workspace.name,
                    Some(index) == active,
                    palette,
                    cx,
                )
            })
            .collect();
        let hidden: Vec<(String, String)> = plan
            .hidden
            .iter()
            .map(|&index| {
                (
                    workspaces[index].workspace_id.clone(),
                    workspaces[index].name.clone(),
                )
            })
            .collect();
        let connected = self.state.connection == Connection::Connected;
        let reconnecting = !connected && !matches!(self.state.connection, Connection::Connecting);

        row()
            .w_full()
            .h(px(layout::TOP_BAR_HEIGHT))
            .flex_none()
            .gap(px(gap))
            .pl(px(leading))
            .pr(px(space::X12))
            .child(self.rail_toggle(palette, cx))
            .children(tabs)
            .child(window_controls::drag_area(spacer()))
            .children((!hidden.is_empty()).then(|| self.overflow(&hidden, palette, cx)))
            .child(self.new_pane_button(connected, palette, cx))
            .children(reconnecting.then(|| reconnecting_note(palette)))
            .children((!shown.is_empty()).then(|| window_controls::controls(&shown, palette, cx)))
    }

    fn rail_toggle(&self, palette: &'static Palette, cx: &mut Context<Self>) -> AnyElement {
        neo::raised(palette, radius::CONTROL, elevation::CARD)
            .id("toggle-rail")
            .size(px(layout::TOP_BAR_CONTROL_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_rail(cx)))
            .child(icon(Icon::Grid, glyph::TOP_BAR_TOGGLE, palette.ink))
            .into_any_element()
    }

    fn tab(
        &self,
        workspace_id: &str,
        name: &str,
        selected: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let select_id = workspace_id.to_owned();
        let close_id = workspace_id.to_owned();
        let surface = if selected {
            neo::well(palette, radius::PILL, desktop::WELL_DEPTH)
        } else {
            neo::raised(palette, radius::PILL, elevation::PILL)
        };
        surface
            .id(id("workspace-tab", workspace_id))
            .h(px(layout::WORKSPACE_TAB_HEIGHT))
            .max_w(px(layout::WORKSPACE_TAB_MAXIMUM_WIDTH))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(space::X6))
            .px(px(space::X12))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.select_workspace(&select_id, cx);
                if let Some(pane_id) = this.state.focused_pane().map(|p| p.pane_id.clone()) {
                    this.focus_input(&pane_id, window, cx);
                }
            }))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(fonts::CHROME)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(type_scale::CAPTION))
                    .text_color(rgb(if selected {
                        palette.accent
                    } else {
                        palette.ink
                    }))
                    .child(SharedString::from(name.to_owned())),
            )
            .child(
                div()
                    .id(id("workspace-close", workspace_id))
                    .flex_none()
                    .p(px(space::X2))
                    .cursor_pointer()
                    .own_press()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.close_workspace(&close_id, cx);
                        cx.stop_propagation();
                    }))
                    .child(icon(Icon::Close, glyph::TAB_CLOSE, palette.muted)),
            )
            .into_any_element()
    }

    fn overflow(
        &mut self,
        hidden: &[(String, String)],
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menus.is_open(&Menu::Overflow);
        let rows: Vec<AnyElement> = hidden
            .iter()
            .map(|(workspace_id, name)| {
                let picked = workspace_id.clone();
                menus::on_pick(
                    menus::item(palette, workspace_id, name.clone(), false),
                    cx,
                    move |this, cx| {
                        this.select_workspace(&picked, cx);
                        this.close_menu(cx);
                    },
                )
            })
            .collect();
        div()
            .relative()
            .flex_none()
            .child(
                neo::raised(palette, radius::PILL, elevation::PILL)
                    .id("workspace-tabs-overflow")
                    .w(px(layout::WORKSPACE_OVERFLOW_BUTTON_WIDTH))
                    .h(px(layout::TOP_BAR_CONTROL_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .font_family(fonts::CHROME)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(type_scale::CAPTION))
                    .text_color(rgb(palette.accent))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_menu(Menu::Overflow, cx)
                    }))
                    .child(format!("+{}", hidden.len())),
            )
            .when(open, |slot| {
                slot.child(menus::popup(
                    palette,
                    Anchor::TopRight,
                    (
                        layout::WORKSPACE_OVERFLOW_BUTTON_WIDTH,
                        layout::TOP_BAR_CONTROL_HEIGHT + space::X6,
                    ),
                    menus::list(rows),
                    cx,
                ))
            })
            .into_any_element()
    }

    fn new_pane_button(
        &mut self,
        connected: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menus.is_open(&Menu::NewPane);
        let rows = if open {
            let pick: Pick =
                Rc::new(|this, harness, cx| this.open_pane(Some(harness.to_owned()), cx));
            let agents = self.harness_rows(None, palette, &pick, cx);
            std::iter::once(menus::heading(palette, "Open a pane").into_any_element())
                .chain(std::iter::once(menus::on_pick(
                    menus::item(palette, "terminal", "Terminal", false),
                    cx,
                    |this, cx| this.open_pane(None, cx),
                )))
                .chain(agents)
                .collect()
        } else {
            Vec::new()
        };
        div()
            .relative()
            .flex_none()
            .child(
                neo::raised(palette, radius::PILL, elevation::PILL)
                    .id("new-pane")
                    .size(px(layout::TOP_BAR_CONTROL_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(!connected, |button| {
                        button.opacity(pane_interior::DISABLED_OPACITY)
                    })
                    .when(connected, |button| button.cursor_pointer())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if connected {
                            this.toggle_menu(Menu::NewPane, cx);
                        }
                    }))
                    .child(icon(Icon::Plus, glyph::TOP_BAR_NEW_PANE, palette.ink)),
            )
            .when(open, |slot| {
                slot.child(menus::popup(
                    palette,
                    Anchor::TopRight,
                    (
                        layout::TOP_BAR_CONTROL_HEIGHT,
                        layout::TOP_BAR_CONTROL_HEIGHT + space::X6,
                    ),
                    menus::list(rows),
                    cx,
                ))
            })
            .into_any_element()
    }
}

fn reconnecting_note(palette: &Palette) -> AnyElement {
    row()
        .flex_none()
        .gap(px(space::X6))
        .child(
            div()
                .size(px(desktop::STATUS_DOT))
                .rounded_full()
                .bg(rgb(palette.warning())),
        )
        .child(
            div()
                .font_family(fonts::MONO)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(type_scale::MICRO))
                .text_color(rgb(palette.secondary))
                .child("Reconnecting"),
        )
        .into_any_element()
}
