//! Popup menus: a raised card that opens beside the control that asked for it and closes when
//! the person clicks anywhere else.

use super::ui::{OwnPress, Typeface, bounds_probe, column, icon, id, row};
use crate::{
    assets::Icon,
    neo,
    popup_fit::{self, Extent, Side, Want},
    theme::{Palette, desktop, elevation, layout, pane_interior, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    Anchor, Anchored, AnyElement, App, ClickEvent, Context, Div, FontWeight, Global, Pixels, Point,
    SharedString, Stateful, anchored, deferred, div, point, prelude::*, px, rgb,
};

/// What the popups of a frame know of the window: how tall it is, and where the control of the
/// popup that is open was laid out (a popup reports it from its first frame, and the window
/// draws the next with it).
#[derive(Default)]
struct Frame {
    window_height: Option<f32>,
    control: Option<Extent>,
}

impl Global for Frame {}

/// Tells the popups how tall the window is. Called by every frame of the window.
pub fn window_measured(height: f32, cx: &mut App) {
    cx.default_global::<Frame>().window_height = Some(height);
}

/// Forgets where the control of the last popup was: the next popup is of another control.
pub fn control_forgotten(cx: &mut App) {
    cx.default_global::<Frame>().control = None;
}

/// `body` opened below or beside the element it is placed in, as tall as the Mac's popover at
/// most. The element must be `relative`.
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

/// As [`popup`], for a card that may grow to `max_height` before it scrolls. The card is cut to
/// the room the window has on the side it opens on, and opens on the other side of its element
/// when there is more room there (`popup_fit`).
pub fn popup_up_to(
    palette: &Palette,
    anchor: Anchor,
    offset: (f32, f32),
    max_height: f32,
    body: Div,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let prefer = if matches!(anchor, Anchor::BottomLeft | Anchor::BottomRight) {
        Side::Above
    } else {
        Side::Below
    };
    let want = Want {
        prefer,
        offset: offset.1,
        height: max_height,
        gap: popup_fit::GAP,
    };
    let frame = cx.default_global::<Frame>();
    let fit = popup_fit::fit(want, frame.control, frame.window_height.unwrap_or(f32::MAX));
    let anchor = if fit.side == prefer {
        anchor
    } else {
        across(anchor)
    };
    let workbench = cx.entity();
    let probe = bounds_probe(move |bounds, cx| {
        let top = f32::from(bounds.origin.y);
        let extent = Extent {
            top,
            bottom: top + f32::from(bounds.size.height),
        };
        let frame = cx.default_global::<Frame>();
        if frame.control != Some(extent) {
            frame.control = Some(extent);
            workbench.update(cx, |_, cx| cx.notify());
        }
    });
    // The overlay is as large as the element, so that the probe measures it; the card is
    // placed from the element's top left corner.
    float(
        palette,
        anchored()
            .anchor(anchor)
            .offset(point(px(offset.0), px(fit.offset))),
        (fit.max_height, body),
        cx,
    )
    .size_full()
    .child(probe)
    .into_any_element()
}

/// The card of `body`, `max_height` tall at most, placed by `place` above everything else and
/// kept inside the window, out of the flow of the element it is in. The element it is returned
/// into must be `relative`.
fn float(
    palette: &Palette,
    place: Anchored,
    (max_height, body): (f32, Div),
    cx: &mut Context<Workbench>,
) -> Div {
    div().absolute().top_0().left_0().child(deferred(
        place
            .snap_to_window_with_margin(px(popup_fit::MARGIN))
            .child(card(palette, max_height, body, cx)),
    ))
}

/// `anchor` on the other side of the element: the corner above for one below, and the reverse.
fn across(anchor: Anchor) -> Anchor {
    match anchor {
        Anchor::TopLeft => Anchor::BottomLeft,
        Anchor::TopRight => Anchor::BottomRight,
        Anchor::BottomLeft => Anchor::TopLeft,
        Anchor::BottomRight => Anchor::TopRight,
        other => other,
    }
}

/// `body` opened at a place in the window, as a context menu is: the pointer's. It opens below
/// the place, or above it when there is more room there.
pub fn popup_at(
    palette: &Palette,
    at: Point<Pixels>,
    body: Div,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let y = f32::from(at.y);
    let window_height = cx.default_global::<Frame>().window_height;
    let fit = popup_fit::fit(
        Want {
            prefer: Side::Below,
            offset: 0.0,
            height: layout::WORKSPACE_OVERFLOW_POPOVER_MAXIMUM_HEIGHT,
            gap: 0.0,
        },
        Some(Extent { top: y, bottom: y }),
        window_height.unwrap_or(f32::MAX),
    );
    let corner = match fit.side {
        Side::Below => Anchor::TopLeft,
        Side::Above => Anchor::BottomLeft,
    };
    float(
        palette,
        anchored().position(at).anchor(corner),
        (fit.max_height, body),
        cx,
    )
    .into_any_element()
}

/// The card of the approved harness picker: radius 20 and the panel elevation.
fn card(
    palette: &Palette,
    max_height: f32,
    body: Div,
    cx: &mut Context<Workbench>,
) -> Stateful<Div> {
    neo::raised(palette, radius::CARD, elevation::PANEL)
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
        .child(body)
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

/// A row that runs a command, with its shortcut at the trailing edge. `enabled` false draws it
/// dimmed and without hover or a pointer.
pub fn command(
    palette: &Palette,
    key: &str,
    label: impl Into<SharedString>,
    shortcut: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    row_frame(palette, key, false, enabled)
        .gap(px(space::X18))
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
        .child(
            div()
                .flex_none()
                .text_color(rgb(palette.muted))
                .text_size(px(type_scale::SMALL))
                .child(shortcut.into()),
        )
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
        .child(super::ui::disclosure_chevron(expanded, palette.muted))
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

/// The recent folders as rows, with the one at `chosen` marked; a row does `pick` with its path.
/// `key` names the rows among the window's.
pub fn folder_rows(
    (key, chosen): (&str, &str),
    palette: &Palette,
    folders: Vec<soyeht_client::state::RecentFolder>,
    cx: &mut Context<Workbench>,
    pick: fn(&mut Workbench, &str, &mut Context<Workbench>),
) -> Vec<AnyElement> {
    let mut rows = Vec::new();
    if !folders.is_empty() {
        rows.push(heading(palette, "Recent folders").into_any_element());
    }
    for folder in folders {
        let path = folder.path.clone();
        let is_chosen = folder.path == chosen;
        rows.push(on_pick(
            detail_item(
                palette,
                &id(key, &folder.path),
                div().into_any_element(),
                folder.name,
                folder.path,
                is_chosen,
                true,
            ),
            cx,
            move |this, cx| pick(this, &path, cx),
        ));
    }
    rows
}
