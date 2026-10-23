//! Changing how the panes are arranged: splitting a pane, and dragging or double-clicking a
//! divider as the Mac does (`PaneDividerEditing`). The rules that decide the new weights are
//! `soyeht_client::layout`'s; this keeps the gesture and carries out what the replica asks for.

use crate::{destination::Destination, neo, workbench::Workbench};
use gpui::{Context, Pixels, Point};
use soyeht_client::layout;
use soyeht_protocol::{WorkspacePaneLayoutAxis as Axis, WorkspacePaneLayoutNode as Node};

const NOT_RESIZED: &str = "Couldn't resize the panes. The layout was not changed.";
const NOT_EQUALIZED: &str = "Couldn't equalize the panes. The layout was not changed.";

/// The divider after child `divider` of the split at `path`: the same in every frame, so a
/// drag keeps its divider while the panes move under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DividerKey {
    pub(crate) path: Vec<usize>,
    pub(crate) divider: usize,
}

/// A divider being dragged: where the pointer went down and the arrangement the drag started
/// from, which every move of the pointer resizes by the whole distance (not by the last step).
struct Gesture {
    key: DividerKey,
    axis: Axis,
    start: Point<Pixels>,
    base: Node,
}

/// What the pointer is dragging in the arrangement of the panes: a divider or a pane.
#[derive(Default)]
pub(crate) struct Gestures {
    gesture: Option<Gesture>,
    pub(crate) redock: crate::redock::Redock,
}

impl Workbench {
    /// Splits a pane: the new pane opens beside it, of the same kind.
    pub(crate) fn split_pane(&mut self, pane_id: &str, axis: Axis, cx: &mut Context<Self>) {
        let harness = self
            .state
            .pane(pane_id)
            .and_then(|pane| pane.agent.as_ref().map(|agent| agent.harness.clone()));
        let ticket = self.next_ticket();
        if let Some(workspace_id) = self.state.expect_split(ticket, pane_id, axis) {
            self.request_pane(workspace_id, harness, ticket, cx);
        }
    }

    /// Whether the panes can be rearranged, by their dividers or their headers: the arrangement
    /// stored for a workspace is of all its panes, which only the agents' place shows.
    pub(crate) fn arrangement_editable(&self) -> bool {
        self.destination == Destination::Agents
    }

    /// The pointer went down on a divider: remember the arrangement the drag starts from.
    pub(crate) fn divider_pressed(&mut self, key: DividerKey, axis: Axis, start: Point<Pixels>) {
        self.state.cancel_layout_draft();
        self.stage.gestures.gesture = self.arrangement().map(|(_, base)| Gesture {
            key,
            axis,
            start,
            base,
        });
    }

    /// The pointer moved with a divider held: show the arrangement that distance makes.
    pub(crate) fn divider_dragged(
        &mut self,
        key: &DividerKey,
        pointer: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (Some(workspace_id), Some(gesture)) =
            (self.workspace_id(), &self.stage.gestures.gesture)
        else {
            return;
        };
        if &gesture.key != key {
            return;
        }
        let delta = match gesture.axis {
            Axis::Horizontal => pointer.x - gesture.start.x,
            Axis::Vertical => pointer.y - gesture.start.y,
        };
        let resized = layout::resize(
            &gesture.base,
            &key.path,
            key.divider,
            f64::from(f32::from(delta)),
            self.stage_size(),
            &crate::views::sizing(),
        );
        if let Ok(tree) = resized {
            self.state.draft_layout(&workspace_id, tree);
        } else {
            // The person is told; the drag is over and the stored arrangement shows again.
            self.drop_gesture(NOT_RESIZED);
        }
        cx.notify();
    }

    /// The pointer let go: store the arrangement the drag made.
    pub(crate) fn divider_released(&mut self, cx: &mut Context<Self>) {
        if self.stage.gestures.gesture.take().is_some() {
            let effects = self.state.commit_layout_draft();
            self.run_effects(effects);
            cx.notify();
        }
    }

    /// A double click: the panes of the split share the room equally.
    pub(crate) fn divider_equalized(&mut self, key: &DividerKey, cx: &mut Context<Self>) {
        self.stage.gestures.gesture = None;
        self.state.cancel_layout_draft();
        let Some((workspace_id, tree)) = self.arrangement() else {
            return;
        };
        let equal = layout::equalize(&tree, &key.path, self.stage_size(), &crate::views::sizing());
        if let Ok(equal) = equal {
            self.state.draft_layout(&workspace_id, equal);
            let effects = self.state.commit_layout_draft();
            self.run_effects(effects);
        } else {
            self.drop_gesture(NOT_EQUALIZED);
        }
        cx.notify();
    }

    fn drop_gesture(&mut self, message: &str) {
        self.stage.gestures.gesture = None;
        self.state.cancel_layout_draft();
        self.state.problem = Some(message.to_owned());
    }

    /// The selected workspace and the arrangement of all its panes on screen.
    fn arrangement(&self) -> Option<(String, Node)> {
        let workspace_id = self.workspace_id()?;
        let tree = self.state.layout(&workspace_id, |_| true)?;
        Some((workspace_id, tree))
    }

    fn workspace_id(&self) -> Option<String> {
        self.state
            .selected_workspace()
            .map(|workspace| workspace.workspace_id.clone())
    }

    /// The room the panes are drawn in (width, height): the stage without the halo around it.
    fn stage_size(&self) -> (f64, f64) {
        let size = self.stage.scroll.bounds().size;
        let inner = |extent: Pixels| f64::from((f32::from(extent) - 2.0 * neo::PANE_HALO).max(0.0));
        (inner(size.width), inner(size.height))
    }
}
