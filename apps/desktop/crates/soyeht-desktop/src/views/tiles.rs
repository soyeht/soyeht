//! The panes of a workspace laid out as its split tree says: a branch is a row or a column of
//! children that share the room by weight, a leaf is a pane card.

use super::ui::{column, icon};
use crate::{
    assets::Icon,
    theme::{Palette, desktop, pane_layout, space, type_scale},
    workbench::Workbench,
};
use gpui::{AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, px, rgb};
use soyeht_protocol::{Pane, WorkspacePaneLayoutAxis, WorkspacePaneLayoutNode as Node};

impl Workbench {
    pub(super) fn tiles(
        &mut self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(workspace) = self.state.selected_workspace() else {
            return empty(
                palette,
                None,
                super::connection_notice(&self.state.connection).hint,
            );
        };
        let workspace_id = workspace.workspace_id.clone();
        let place = self.destination;
        if !place.is_pane_grid() {
            return empty(
                palette,
                Some(place.icon()),
                &format!(
                    "{} is not available in the desktop client yet.",
                    place.label()
                ),
            );
        }
        let Some(tree) = self
            .state
            .layout(&workspace_id, |pane| place.shows(&pane.mode))
        else {
            let message = if self.state.panes_of(&workspace_id).next().is_none() {
                "This workspace has no panes yet. Use + to open one."
            } else {
                "No panes of this kind in the workspace."
            };
            return empty(palette, Some(place.icon()), message);
        };
        let focused = self.state.focused_pane().map(|pane| pane.pane_id.clone());
        self.tile(&tree, focused.as_deref(), palette, cx)
    }

    fn tile(
        &mut self,
        node: &Node,
        focused: Option<&str>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::V0(leaf) => {
                let pane: Option<Pane> = self
                    .state
                    .panes()
                    .iter()
                    .find(|pane| pane.pane_id == leaf.pane_id)
                    .cloned();
                match pane {
                    Some(pane) => {
                        let is_focused = focused == Some(pane.pane_id.as_str());
                        self.pane_card(&pane, is_focused, palette, cx)
                    }
                    None => div().flex_1().into_any_element(),
                }
            }
            Node::V1(split) => {
                let branches: Vec<AnyElement> = split
                    .children
                    .iter()
                    .map(|branch| {
                        let child = self.tile(&branch.node, focused, palette, cx);
                        let mut share = div().flex().flex_1().min_w_0().min_h_0();
                        share.style().flex_grow = Some(branch.weight as f32);
                        share.child(child).into_any_element()
                    })
                    .collect();
                let layout = div().flex_1().min_w_0().min_h_0().flex();
                let layout = match split.axis {
                    WorkspacePaneLayoutAxis::Horizontal => layout.flex_row(),
                    WorkspacePaneLayoutAxis::Vertical => layout.flex_col(),
                };
                layout
                    .gap(px(pane_layout::PANE_GAP))
                    .children(branches)
                    .into_any_element()
            }
        }
    }
}

/// A calm message where the panes would be, with the icon of the place when it has one.
fn empty(palette: &Palette, glyph: Option<Icon>, message: &str) -> AnyElement {
    column()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(space::X12))
        .text_size(px(type_scale::CAPTION))
        .text_color(rgb(palette.muted))
        .children(glyph.map(|glyph| icon(glyph, desktop::EMPTY_STATE_GLYPH, palette.muted)))
        .child(SharedString::from(message.to_owned()))
        .into_any_element()
}
