//! Moving a pane by dragging its header onto another pane, as the Mac does (`PaneDragDrop`): which
//! zone of which pane the pointer is over while the drag lasts, and what a drop and Escape do.
//! The zones and the new arrangement are `soyeht_client::layout`'s; this keeps the gesture.

use crate::workbench::Workbench;
use gpui::{Bounds, Context, DragMoveEvent, KeyDownEvent, Pixels, SharedString, Window};
use soyeht_client::layout::{Rect, Zone, zone_at};
use soyeht_client::state::problem::kind;

const NOT_MOVED: &str = "Couldn't move the pane. The layout was not changed.";

/// What a header drag carries: the pane being moved, and its name for the ghost beside the pointer.
#[derive(Clone)]
pub(crate) struct PaneDrag {
    pub(crate) pane_id: String,
    pub(crate) title: SharedString,
}

/// The zone of a pane that a drop would land on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Over {
    pane_id: String,
    pub(crate) zone: Zone,
    /// The width and height of the pane's card, which the zone is a part of.
    pub(crate) size: (f32, f32),
}

/// A pane being dragged and the zone it is over.
#[derive(Default)]
pub(crate) struct Redock {
    /// The pane being dragged, from the first move of the pointer until the drag is over.
    source: Option<String>,
    /// The zone under the pointer. Never one of the pane being dragged: a pane dropped on itself
    /// changes nothing.
    over: Option<Over>,
}

fn rect_of(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        x: f64::from(f32::from(bounds.origin.x)),
        y: f64::from(f32::from(bounds.origin.y)),
        width: f64::from(f32::from(bounds.size.width)),
        height: f64::from(f32::from(bounds.size.height)),
    }
}

impl Workbench {
    /// The pointer moved with a header held over the card of `target`.
    pub(crate) fn pane_dragged(
        &mut self,
        target: &str,
        event: &DragMoveEvent<PaneDrag>,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = event.dragged_item().downcast_ref::<PaneDrag>() else {
            return;
        };
        let redock = &mut self.stage.gestures.redock;
        redock.source = Some(drag.pane_id.clone());
        let pointer = (
            f64::from(f32::from(event.event.position.x)),
            f64::from(f32::from(event.event.position.y)),
        );
        let zone = zone_at(rect_of(event.bounds), pointer).filter(|_| drag.pane_id != target);
        let here = zone.map(|zone| Over {
            pane_id: target.to_owned(),
            zone,
            size: (
                f32::from(event.bounds.size.width),
                f32::from(event.bounds.size.height),
            ),
        });
        let changed = if here.is_some() {
            redock.over != here
        } else {
            redock
                .over
                .as_ref()
                .is_some_and(|over| over.pane_id == target)
        };
        if changed {
            redock.over = here;
            cx.notify();
        }
    }

    /// The zone of the pane `pane_id` that a drop would land on, to light up.
    pub(crate) fn drop_zone_over(&self, pane_id: &str) -> Option<&Over> {
        self.stage
            .gestures
            .redock
            .over
            .as_ref()
            .filter(|over| over.pane_id == pane_id)
    }

    /// The header was let go over the card of `target`: the pane goes where the zone says, and
    /// the new arrangement is stored.
    pub(crate) fn pane_dropped(&mut self, target: &str, drag: &PaneDrag, cx: &mut Context<Self>) {
        let zone = self.drop_zone_over(target).map(|over| over.zone);
        self.stage.gestures.redock = Redock::default();
        if let Some(zone) = zone {
            if let Ok(effects) = self.state.move_pane(&drag.pane_id, target, zone) {
                self.run_effects(effects);
            } else {
                self.state.report(kind::LAYOUT_NOT_MOVED, NOT_MOVED);
            }
        }
        cx.notify();
    }

    /// The pointer was let go anywhere a pane cannot be dropped: the drag is over.
    pub(crate) fn pane_drag_ended(&mut self, cx: &mut Context<Self>) {
        if self.stage.gestures.redock.source.take().is_some() {
            self.stage.gestures.redock = Redock::default();
            cx.notify();
        }
    }

    /// Escape during the drag of a pane cancels it: nothing is dropped and the arrangement stays.
    pub(crate) fn pane_drag_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" && self.stage.gestures.redock.source.is_some() {
            cx.stop_active_drag(window);
            self.stage.gestures.redock = Redock::default();
            cx.notify();
            cx.stop_propagation();
        }
    }
}
