//! Popup menus: a raised card that opens beside the control that asked for it and closes when
//! the person clicks anywhere else.

use super::ui::{OwnPress, column, id, row};
use crate::{
    neo,
    theme::{Palette, desktop, elevation, fonts, layout, pane_interior, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, anchored, deferred,
    div, point, prelude::FluentBuilder, px, rgb,
};

/// `body` opened below or beside the element it is placed in. The element must be `relative`.
pub fn popup(
    palette: &Palette,
    anchor: Anchor,
    offset: (f32, f32),
    body: Div,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let card = neo::raised(palette, radius::CONTROL, elevation::BALLOON)
        .flex()
        .flex_col()
        .p(px(space::X6))
        .min_w(px(desktop::MENU_MIN_WIDTH))
        // The Mac's popover height: a longer list scrolls inside the card, and the card snaps
        // into the window.
        .max_h(px(layout::WORKSPACE_OVERFLOW_POPOVER_MAXIMUM_HEIGHT))
        .id("menu")
        .overflow_y_scroll()
        .occlude()
        .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_menu(cx)))
        .child(body);
    div()
        .absolute()
        .top_0()
        .left_0()
        .child(deferred(
            anchored()
                .anchor(anchor)
                .offset(point(px(offset.0), px(offset.1)))
                .snap_to_window_with_margin(px(space::X8))
                .child(card),
        ))
        .into_any_element()
}

/// The frame of a row: the check mark column, hover and the padding. `enabled` false draws it
/// dimmed and without hover or a pointer.
fn row_frame(palette: &Palette, key: &str, checked: bool, enabled: bool) -> Stateful<Div> {
    row()
        .id(id("menu-item", key))
        .gap(px(space::X8))
        .px(px(space::X10))
        .py(px(space::X6))
        .rounded(px(radius::COMPACT))
        .text_size(px(type_scale::CAPTION))
        .text_color(rgb(palette.ink))
        .when(enabled, |row| {
            row.cursor_pointer()
                .hover(|style| style.bg(rgb(palette.hover)))
        })
        .when(!enabled, |row| row.opacity(pane_interior::DISABLED_OPACITY))
        .child(
            div()
                .w(px(space::X12))
                .flex_none()
                .text_color(rgb(palette.accent))
                .child(if checked { "\u{2713}" } else { "" }),
        )
}

/// A selectable row of a menu.
pub fn item(
    palette: &Palette,
    key: &str,
    label: impl Into<SharedString>,
    checked: bool,
) -> Stateful<Div> {
    row_frame(palette, key, checked, true)
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
}

/// A row with a second, smaller line under its label, such as how ready an agent is, so that
/// two rows with the same label can be told apart.
pub fn detail_item(
    palette: &Palette,
    key: &str,
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    checked: bool,
    enabled: bool,
) -> Stateful<Div> {
    row_frame(palette, key, checked, enabled).child(
        column()
            .flex_1()
            .min_w_0()
            .child(div().truncate().child(label.into()))
            .child(
                div()
                    .truncate()
                    .text_size(px(type_scale::SMALL))
                    .text_color(rgb(palette.muted))
                    .child(detail.into()),
            ),
    )
}

/// A small heading above a group of rows.
pub fn heading(palette: &Palette, text: &str) -> Div {
    div()
        .px(px(space::X10))
        .pt(px(space::X6))
        .pb(px(space::X4))
        .font_family(fonts::CHROME)
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.muted))
        .child(text.to_uppercase())
}

/// A row of plain text that is not selectable.
pub fn note(palette: &Palette, text: impl Into<SharedString>) -> Div {
    div()
        .px(px(space::X10))
        .py(px(space::X6))
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(palette.secondary))
        .child(text.into())
}

/// Items in a column.
pub fn list(items: Vec<AnyElement>) -> Div {
    column().gap(px(space::X2)).children(items)
}

/// Runs `on_pick` when the item is clicked, then closes nothing itself: the handler decides.
pub fn on_pick(
    item: Stateful<Div>,
    cx: &mut Context<Workbench>,
    handler: impl Fn(&mut Workbench, &mut Context<Workbench>) + 'static,
) -> AnyElement {
    item.own_press()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| handler(this, cx)))
        .into_any_element()
}
