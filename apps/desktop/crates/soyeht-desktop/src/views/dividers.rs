//! The space between two panes of a split: the grip that resizes them. Drag it to move the
//! divider, double-click it to give the panes equal shares. The hit area is the whole gap and
//! never overlaps a pane, as on the Mac (`PaneLayoutDividerHandle`).

use super::prelude::*;
use crate::{layout_edit::DividerKey, theme::pane_layout};
use gpui::{
    App, AppContext, CursorStyle, DragMoveEvent, MouseButton, MouseDownEvent, MouseUpEvent, Render,
    Window, px,
};
use soyeht_protocol::WorkspacePaneLayoutAxis as Axis;

/// What a divider drag carries, so that GPUI sends every move of the pointer to the dividers
/// while it lasts, wherever the pointer is.
#[derive(Clone)]
struct DividerDrag;

/// What GPUI shows beside the pointer for a drag: nothing; the panes show the move.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The pointer let go of the divider, on it or off it.
fn released(cx: &Context<Workbench>) -> impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static {
    cx.listener(|this, _, _, cx| this.divider_released(cx))
}

impl Workbench {
    /// The gap after child `divider` of the split at `path`, which grips when the panes can be
    /// resized from here.
    pub(super) fn divider(
        &self,
        path: &[usize],
        divider: usize,
        axis: &Axis,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let gap = div().flex_none();
        let gap = match axis {
            Axis::Horizontal => gap.w(px(pane_layout::PANE_GAP)),
            Axis::Vertical => gap.h(px(pane_layout::PANE_GAP)),
        };
        if !self.arrangement_editable() {
            return gap.into_any_element();
        }
        let key = DividerKey {
            path: path.to_vec(),
            divider,
        };
        let name = path
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join("-");
        let cursor = match axis {
            Axis::Horizontal => CursorStyle::ResizeLeftRight,
            Axis::Vertical => CursorStyle::ResizeUpDown,
        };
        let (pressed, dragged, equalized) = (key.clone(), key.clone(), key);
        let pressed_axis = axis.clone();
        gap.id(id("divider", &format!("{name}:{divider}")))
            .cursor(cursor)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                    this.divider_pressed(pressed.clone(), pressed_axis.clone(), event.position);
                }),
            )
            .on_drag(DividerDrag, |_, _, _, cx| cx.new(|_| NoGhost))
            .on_drag_move::<DividerDrag>(cx.listener(
                move |this, event: &DragMoveEvent<DividerDrag>, _, cx| {
                    this.divider_dragged(&dragged, event.event.position, cx);
                },
            ))
            .on_mouse_up(MouseButton::Left, released(cx))
            .on_mouse_up_out(MouseButton::Left, released(cx))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.divider_equalized(&equalized, cx);
                }
            }))
            .into_any_element()
    }
}
