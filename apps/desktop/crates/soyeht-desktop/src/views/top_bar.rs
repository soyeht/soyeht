//! The top bar of the Mac window: the rail toggle, the workspace tabs as rounded pills with a
//! close mark, a "+N" pill for the tabs that do not fit, the "+" that opens a pane, and on
//! Linux and Windows the window controls.

use super::{
    harness_menu::{Pick, Picker},
    menus,
    ui::{
        OwnPress, Typeface, edge_fades, hidden_ends, hint, icon, id, row, spacer, text_width,
        top_bar_button, waiting_badge, working_dot,
    },
    window_controls,
};
use crate::{
    assets::Icon,
    neo,
    open_menu::Menu,
    theme::{Palette, desktop, elevation, glyph, layout, pane_interior, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, point,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let gap = space::X10;
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
        let strip = self.tab_strip(palette, window, cx);
        let hidden = strip.hidden;
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
            .child(strip.element)
            .child(window_controls::drag_area(spacer().min_w(px(MINIMUM_DRAG))))
            .children((!hidden.is_empty()).then(|| self.overflow(&hidden, palette, cx)))
            .child(self.new_pane_button(connected, palette, cx))
            .children(reconnecting.then(|| reconnecting_note(palette)))
            .children((!shown.is_empty()).then(|| window_controls::controls(&shown, palette, cx)))
    }

    /// The workspace tabs in a strip that scrolls sideways, as the Mac's does: a fade over each end
    /// that hides tabs, with a chevron on it that moves the strip to the next hidden tab, and the
    /// active tab scrolled into view when it changes. The tabs that are not wholly in view, as
    /// the last frame laid them out, are the ones the "+N" button lists.
    fn tab_strip(
        &mut self,
        palette: &'static Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> TabStrip {
        let workspaces = self.state.workspaces();
        let ids: Vec<(String, String)> = workspaces
            .iter()
            .map(|w| (w.workspace_id.clone(), w.name.clone()))
            .collect();
        let selected = self
            .state
            .selected_workspace()
            .map(|w| w.workspace_id.clone());
        let active = ids.iter().position(|(id, _)| Some(id) == selected.as_ref());
        let viewport = self.tab_scroll.bounds();
        // The active tab is brought into view by the smallest move that shows all of it, once the
        // strip has been laid out and its tab has a place.
        if self.revealed_tab != selected
            && viewport.size.width > px(0.0)
            && let Some(tab) = active.and_then(|index| self.tab_scroll.bounds_for_item(index))
        {
            let clearance = px(shadow_clearance());
            let shift = if tab.left() < viewport.left() + clearance {
                viewport.left() + clearance - tab.left()
            } else if tab.right() > viewport.right() - clearance {
                viewport.right() - clearance - tab.right()
            } else {
                px(0.0)
            };
            let offset = self.tab_scroll.offset();
            self.tab_scroll
                .set_offset(point(offset.x + shift, offset.y));
            self.revealed_tab.clone_from(&selected);
        }
        let hidden: Vec<usize> = (0..ids.len())
            .filter(|&index| {
                self.tab_scroll.bounds_for_item(index).is_some_and(|tab| {
                    viewport.size.width > px(0.0)
                        && (tab.left() < viewport.left() || tab.right() > viewport.right())
                })
            })
            .collect();
        if hidden.len() != self.hidden_tabs {
            // The "+N" button changes the strip's room: lay it out again with the new count.
            self.hidden_tabs = hidden.len();
            cx.notify();
        }
        let (left, right) = hidden_ends(&self.tab_scroll);
        let first_hidden = hidden.first().copied();
        let last_hidden = hidden.last().copied();
        let tabs: Vec<AnyElement> = ids
            .iter()
            .enumerate()
            .map(|(index, (id, name))| {
                self.tab(id, name, Some(index) == active, palette, window, cx)
            })
            .collect();
        let clearance = shadow_clearance();
        let chevron = |key: &'static str, glyph: Icon, target: Option<usize>| {
            row()
                .id(key)
                .absolute()
                .top_0()
                .bottom_0()
                .when(key == "tabs-scroll-left", |end| end.left(px(space::X2)))
                .when(key == "tabs-scroll-right", |end| end.right(px(space::X2)))
                .cursor_pointer()
                .own_press()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(index) = target {
                        this.tab_scroll.scroll_to_item(index);
                        cx.notify();
                    }
                }))
                .child(icon(glyph, glyph::CHEVRON, palette.ink))
                .into_any_element()
        };
        let mut strip = div()
            .id("workspace-tabs")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(space::X10))
            .p(px(clearance))
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .children(tabs);
        strip.style().flex_shrink = Some(1.0);
        let element = div()
            .relative()
            .flex()
            .flex_shrink(1.0)
            .min_w_0()
            .h(px(layout::WORKSPACE_TAB_HEIGHT + 2.0 * clearance))
            .child(strip)
            .children(edge_fades(&self.tab_scroll, palette))
            .children(left.then(|| chevron("tabs-scroll-left", Icon::ChevronLeft, first_hidden)))
            .children(right.then(|| chevron("tabs-scroll-right", Icon::ChevronRight, last_hidden)))
            .into_any_element();
        TabStrip {
            element,
            hidden: hidden.into_iter().map(|index| ids[index].clone()).collect(),
        }
    }

    fn rail_toggle(&self, palette: &'static Palette, cx: &mut Context<Self>) -> AnyElement {
        top_bar_button(
            palette,
            radius::CONTROL,
            elevation::CARD,
            layout::TOP_BAR_CONTROL_HEIGHT,
        )
        .id("toggle-rail")
        .cursor_pointer()
        .tooltip(hint("Toggle app bar", palette))
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Beside the name: how many requests wait in the workspace, else a dot while an agent
        // of it works.
        let waiting = self.state.waiting_requests(Some(workspace_id));
        let working = self.state.has_working_agent(Some(workspace_id));
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
            // The name is in the tooltip only when the tab cuts it short, as the Mac's `.help(name)`
            // is of use only then: a name that is all in view needs no second reading.
            .when(name_is_cut(name, waiting, working, window), |tab| {
                tab.tooltip(hint(name.to_owned(), palette))
            })
            .child(
                row()
                    .min_w_0()
                    .gap(px(space::X8))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .chrome()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(type_scale::CAPTION))
                            .text_color(rgb(if selected {
                                palette.accent
                            } else {
                                palette.ink
                            }))
                            .child(SharedString::from(name.to_owned())),
                    )
                    .children((waiting > 0).then(|| waiting_badge(waiting, palette)))
                    .children((waiting == 0 && working).then(|| working_dot(palette))),
            )
            .child(
                div()
                    .id(id("workspace-close", workspace_id))
                    .flex_none()
                    .p(px(space::X2))
                    .cursor_pointer()
                    .own_press()
                    .tooltip(hint(format!("Close {name}"), palette))
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
        with_list(
            palette,
            open,
            layout::WORKSPACE_OVERFLOW_BUTTON_WIDTH,
            rows,
            cx,
            top_bar_button(
                palette,
                radius::PILL,
                elevation::PILL,
                layout::WORKSPACE_OVERFLOW_BUTTON_WIDTH,
            )
            .id("workspace-tabs-overflow")
            .cursor_pointer()
            .chrome()
            .font_weight(FontWeight::BOLD)
            .text_size(px(type_scale::CAPTION))
            .text_color(rgb(palette.accent))
            .tooltip(hint(
                format!("Show {} more workspaces", hidden.len()),
                palette,
            ))
            .on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_menu(Menu::Overflow, cx)),
            )
            .child(format!("+{}", hidden.len())),
        )
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
            let agents = self.harness_rows(Picker::default(), palette, &pick, cx);
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
        with_list(
            palette,
            open,
            layout::TOP_BAR_CONTROL_HEIGHT,
            rows,
            cx,
            top_bar_button(
                palette,
                radius::PILL,
                elevation::PILL,
                layout::TOP_BAR_CONTROL_HEIGHT,
            )
            .id("new-pane")
            .when(!connected, |button| {
                button.opacity(pane_interior::DISABLED_OPACITY)
            })
            .when(connected, |button| button.cursor_pointer())
            .tooltip(hint("Open a terminal or agent pane", palette))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if connected {
                    this.toggle_menu(Menu::NewPane, cx);
                }
            }))
            .child(icon(Icon::Plus, glyph::TOP_BAR_NEW_PANE, palette.ink)),
        )
    }
}

/// A button of the top bar and, while `open`, the list of `rows` under its right edge. `width`
/// is the button's width, which puts the list's corner under it.
fn with_list(
    palette: &'static Palette,
    open: bool,
    width: f32,
    rows: Vec<AnyElement>,
    cx: &mut Context<Workbench>,
    button: impl IntoElement,
) -> AnyElement {
    div()
        .relative()
        .flex_none()
        .child(button)
        .when(open, |slot| {
            slot.child(menus::popup(
                palette,
                Anchor::TopRight,
                (width, layout::TOP_BAR_CONTROL_HEIGHT + space::X6),
                menus::list(rows),
                cx,
            ))
        })
        .into_any_element()
}

/// The strip of tabs and the workspaces whose tabs are not wholly in view.
struct TabStrip {
    element: AnyElement,
    hidden: Vec<(String, String)>,
}

/// Whether a tab cuts `name` short. The name has what the widest tab leaves it beside the padding,
/// the close mark and, while there is one, the count of waiting requests or the dot of a working
/// agent; it is cut when its text is wider than that.
fn name_is_cut(name: &str, waiting: usize, working: bool, window: &Window) -> bool {
    let beside = if waiting > 0 {
        space::X8
            + 2.0 * space::X6
            + text_width(
                window,
                &waiting.to_string(),
                type_scale::MICRO,
                FontWeight::BOLD,
            )
    } else if working {
        space::X8 + space::X7
    } else {
        0.0
    };
    let close = 2.0 * space::X2 + glyph::TAB_CLOSE;
    let room = layout::WORKSPACE_TAB_MAXIMUM_WIDTH - 2.0 * space::X12 - space::X6 - close - beside;
    text_width(window, name, type_scale::CAPTION, FontWeight::SEMIBOLD) > room
}

/// How far a raised pill's shadow reaches past its edge: the room a strip that clips its tabs
/// leaves around them.
fn shadow_clearance() -> f32 {
    let pill = elevation::PILL;
    (pill.light_offset + pill.light_blur).max(pill.dark_offset + pill.dark_blur)
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
                .mono()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(type_scale::MICRO))
                .text_color(rgb(palette.secondary))
                .child("Reconnecting"),
        )
        .into_any_element()
}
