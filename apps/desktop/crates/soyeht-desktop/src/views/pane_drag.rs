//! Dragging a pane by its header: the header is the handle, the card of every other pane takes
//! the drop and lights up the zone the pointer is over, and a ghost with the pane's name follows
//! the pointer. The flat accent fill and border are the Mac's (`PaneDropZonesOverlay`).

use super::ui::{icon, id, row};
use crate::{
    assets::Icon,
    neo,
    redock::PaneDrag,
    theme::{Palette, desktop, elevation, glyph, pane_layout, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Div, DragMoveEvent, FontWeight, Hsla, MouseUpEvent};
use gpui::{Pixels, Point, SharedString, Stateful, Window, div, px, rgb};
use soyeht_client::layout::{Rect, zone_rect};
use soyeht_protocol::Pane;

/// What follows the pointer while a pane is dragged: its name in a floating chip.
struct PaneGhost {
    title: SharedString,
    palette: &'static Palette,
    /// Where in the header the pointer took hold: the chip is drawn beside the pointer.
    grip: Point<Pixels>,
}

impl Render for PaneGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // GPUI places this view at the corner of the header's grip, whatever margin it asks for, so
        // the padding is what puts the chip beside the pointer.
        let chip = neo::raised(self.palette, radius::PILL, elevation::CHIP)
            .opacity(desktop::DRAG_GHOST_OPACITY)
            .child(
                row()
                    .h(px(pane_layout::HEADER_HEIGHT))
                    .max_w(px(desktop::PANE_TITLE_MAX_WIDTH))
                    .px(px(space::X12))
                    .gap(px(space::X7))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(type_scale::CONTROL))
                    .text_color(rgb(self.palette.ink))
                    .child(icon(Icon::Columns, glyph::TAB_CLOSE, self.palette.ink))
                    .child(div().min_w_0().truncate().child(self.title.clone())),
            );
        div()
            .pl(self.grip.x + px(space::X12))
            .pt(self.grip.y + px(space::X12))
            .child(chip)
    }
}

/// The pointer was let go anywhere a pane cannot be dropped, on the window or off it.
pub(super) fn ended(
    cx: &Context<Workbench>,
) -> impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static {
    cx.listener(|this, _, _, cx| this.pane_drag_ended(cx))
}

impl Workbench {
    /// A pane's header, which drags the pane when the arrangement can be changed.
    pub(super) fn draggable_header(
        &self,
        header: Div,
        pane: &Pane,
        palette: &'static Palette,
    ) -> AnyElement {
        if !self.arrangement_editable() {
            return header.into_any_element();
        }
        let drag = PaneDrag {
            pane_id: pane.pane_id.clone(),
            title: SharedString::from(pane.name.clone()),
        };
        header
            .id(id("pane-header", &pane.pane_id))
            .on_drag(drag, move |drag, grip, _: &mut Window, cx: &mut App| {
                let (title, grip) = (drag.title.clone(), grip);
                cx.new(|_| PaneGhost {
                    title,
                    palette,
                    grip,
                })
            })
            .into_any_element()
    }

    /// Makes a pane's card take the drop of another pane's header, and light up the zone the
    /// pointer is over.
    pub(super) fn drop_target(
        &self,
        card: Stateful<Div>,
        pane_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (over, dropped, own) = (pane_id.to_owned(), pane_id.to_owned(), pane_id.to_owned());
        card.relative()
            .on_drag_move::<PaneDrag>(cx.listener(
                move |this, event: &DragMoveEvent<PaneDrag>, _, cx| {
                    this.pane_dragged(&over, event, cx);
                },
            ))
            // A pane dropped on its own card changes nothing, as on the Mac: it is not a drop.
            .can_drop(move |dragged, _, _| {
                dragged
                    .downcast_ref::<PaneDrag>()
                    .is_some_and(|drag| drag.pane_id != own)
            })
            .on_drop::<PaneDrag>(cx.listener(move |this, drag: &PaneDrag, _, cx| {
                this.pane_dropped(&dropped, drag, cx);
            }))
            .children(self.drop_zone_over(pane_id).map(|over| {
                // The part of the card the zone covers, in points from the card's corner.
                let part = zone_rect(
                    over.zone,
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: f64::from(over.size.0),
                        height: f64::from(over.size.1),
                    },
                );
                let accent = rgb(palette.accent);
                div()
                    .absolute()
                    .left(px(part.x as f32))
                    .top(px(part.y as f32))
                    .w(px(part.width as f32))
                    .h(px(part.height as f32))
                    .rounded(px(radius::COMPACT))
                    .bg(Hsla::from(accent).opacity(desktop::DROP_ZONE_OPACITY))
                    .border(px(desktop::DROP_BORDER))
                    .border_color(accent)
            }))
    }
}
