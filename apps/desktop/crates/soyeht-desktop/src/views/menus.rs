//! Popup menus: a raised card that opens beside the control that asked for it and closes when
//! the person clicks anywhere else.

use super::ui::{OwnPress, Typeface, column, icon, id, row};
use crate::{
    assets::Icon,
    neo,
    theme::{Palette, desktop, elevation, layout, pane_interior, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, Stateful, anchored,
    deferred, div, point, prelude::*, px, rgb,
};

/// `body` opened below or beside the element it is placed in. The element must be `relative`.
pub fn popup(
    palette: &Palette,
    anchor: Anchor,
    offset: (f32, f32),
    body: Div,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    popup_up_to(
        palette,
        anchor,
        offset,
        layout::WORKSPACE_OVERFLOW_POPOVER_MAXIMUM_HEIGHT,
        body,
        cx,
    )
}

/// As [`popup`], for a card that may grow to `max_height` before it scrolls.
pub fn popup_up_to(
    palette: &Palette,
    anchor: Anchor,
    offset: (f32, f32),
    max_height: f32,
    body: Div,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    // The card of the approved harness picker: radius 20 and the panel elevation.
    let card = neo::raised(palette, radius::CARD, elevation::PANEL)
        .flex()
        .flex_col()
        .px(px(space::X14))
        .py(px(space::X12))
        .min_w(px(desktop::MENU_MIN_WIDTH))
        // The Mac's popover height: a longer list scrolls inside the card, and the card snaps
        // into the window.
        .max_h(px(max_height))
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

/// The frame of a row of the approved menus: a rounded row that fills with the well while it is
/// the chosen one and with the hover color under the pointer. `enabled` false draws it dimmed and
/// without hover or a pointer.
fn row_frame(palette: &Palette, key: &str, checked: bool, enabled: bool) -> Stateful<Div> {
    row()
        .id(id("menu-item", key))
        .gap(px(space::X8))
        .px(px(space::X10))
        .py(px(space::X7))
        .rounded(px(radius::COMPACT))
        .chrome()
        .text_size(px(type_scale::CAPTION))
        .font_weight(if checked {
            FontWeight::BOLD
        } else {
            FontWeight::SEMIBOLD
        })
        .text_color(rgb(if checked {
            palette.ink
        } else {
            palette.secondary
        }))
        .when(checked, |row| row.bg(rgb(palette.well)))
        .when(enabled && !checked, |row| {
            row.cursor_pointer()
                .hover(|style| style.bg(rgb(palette.hover)))
        })
        .when(enabled && checked, |row| row.cursor_pointer())
        .when(!enabled, |row| row.opacity(pane_interior::DISABLED_OPACITY))
}

/// The check at the trailing edge of the chosen row.
fn check(palette: &Palette) -> AnyElement {
    icon(Icon::Check, desktop::MENU_CHECK_GLYPH, palette.accent).into_any_element()
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
        .children(checked.then(|| check(palette)))
}

/// A row of the harness lists, as the Mac's picker draws it: a mark of how ready the harness
/// is, its name, and under it in small type how ready it is, so that two rows with the same
/// name can be told apart.
pub fn detail_item(
    palette: &Palette,
    key: &str,
    mark: AnyElement,
    label: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    checked: bool,
    enabled: bool,
) -> Stateful<Div> {
    row_frame(palette, key, checked, enabled)
        .min_h(px(desktop::MENU_HARNESS_ROW_HEIGHT))
        .child(div().w(px(space::X18)).flex_none().child(mark))
        .child(
            column()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(palette.ink))
                        .child(label.into()),
                )
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(type_scale::SMALL))
                        .text_color(rgb(palette.muted))
                        .child(detail.into()),
                ),
        )
        .children(checked.then(|| check(palette)))
}

/// The face of a heading: small bold chrome type in the muted ink, with room above.
fn heading_face(palette: &Palette, base: Div) -> Div {
    base.pt(px(space::X6))
        .pb(px(space::X4))
        .chrome()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.muted))
}

/// A small heading above a group of rows.
pub fn heading(palette: &Palette, text: &str) -> Div {
    heading_face(palette, div()).child(text.to_uppercase())
}

/// A heading that opens or closes the group under it, with the chevron of its state.
pub fn disclosure(palette: &Palette, key: &str, text: &str, expanded: bool) -> Stateful<Div> {
    heading_face(palette, row().gap(px(space::X6)))
        .id(id("menu-disclosure", key))
        .cursor_pointer()
        .child(text.to_uppercase())
        .child(icon(
            if expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            desktop::DISCLOSURE_GLYPH,
            palette.muted,
        ))
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
