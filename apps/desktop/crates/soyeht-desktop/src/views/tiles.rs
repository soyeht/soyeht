//! The panes of a workspace laid out as its split tree says: a branch is a row or a column of
//! children that share the room by weight, a leaf is a pane card.

use super::ui::empty_state;
use crate::{assets::Icon, neo, pane_floors, theme::Palette, workbench::Workbench};
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, point, px,
};
use soyeht_client::{
    layout::{Sizing, pane_span, reveal_offset},
    transcript_rows,
};
use soyeht_protocol::{
    Pane, WorkspacePaneLayoutAxis, WorkspacePaneLayoutKind as Kind, WorkspacePaneLayoutNode as Node,
};

/// The room a split gives a child: at least the child's floor on both axes, and along the
/// split the floor is also where its share starts, so that a divider dragged by a distance moves
/// by that distance (`soyeht_client::layout::resize`).
fn share(floor: (f64, f64), weight: f64, axis: &WorkspacePaneLayoutAxis) -> gpui::Div {
    let (width, height) = (px(floor.0 as f32), px(floor.1 as f32));
    let share = div()
        .flex()
        .min_w(width)
        .min_h(height)
        .flex_grow(weight as f32)
        .flex_shrink_0();
    match axis {
        WorkspacePaneLayoutAxis::Horizontal => share.flex_basis(width),
        WorkspacePaneLayoutAxis::Vertical => share.flex_basis(height),
    }
}

/// What an arrangement is measured with: the floor of each kind of pane (`pane_floors`), the
/// agent's wide enough for the composer it always shows. An editor needs less room with its
/// project panel hidden. The state holds it (`State::sizing_of`), so the weights of an
/// arrangement and the stage that draws it are measured alike.
pub(crate) fn sizing(project_panel: bool) -> Sizing {
    let mut sizing = pane_floors::sizing(project_panel);
    let composer = f64::from(super::composer::minimum_pane_width());
    for (kind, floor) in &mut sizing.kinds {
        if *kind == Kind::Agent {
            floor.0 = floor.0.max(composer);
        }
    }
    sizing
}

impl Workbench {
    pub(super) fn tiles(
        &mut self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.start.as_ref().is_some_and(|start| start.shown)
            && super::connection_hint(&self.state.connection).is_none()
            && let Some(form) = self.start_form(cx, palette)
        {
            return form;
        }
        let Some(workspace) = self.state.selected_workspace() else {
            return match super::connection_hint(&self.state.connection) {
                Some(hint) => empty(palette, None, hint),
                None => self.welcome(palette, cx),
            };
        };
        let workspace_id = workspace.workspace_id.clone();
        let place = self.destination;
        if place == crate::destination::Destination::Tasks {
            return self.tasks_surface(&workspace_id, palette, cx);
        }
        if let Some(section) = place.section() {
            return self.place_surface(cx, palette, &workspace_id, &section);
        }
        self.places.leave(None);
        if !place.is_pane_grid() {
            return empty(
                palette,
                Some(place.icon()),
                &super::ui::not_available(place.label()),
            );
        }
        let Some(tree) = self
            .state
            .layout(&workspace_id, |pane| place.shows(&pane.mode))
        else {
            if place == crate::destination::Destination::Editor {
                return self.editor_empty(cx, palette, &workspace_id);
            }
            if place == crate::destination::Destination::Web {
                return self.web_empty(cx, palette);
            }
            let message = if self.state.panes_of(&workspace_id).next().is_none() {
                "This workspace has no panes yet. Use + to open one."
            } else {
                "No panes of this kind in the workspace."
            };
            return empty(palette, Some(place.icon()), message);
        };
        let focused = self.state.focused_pane().map(|pane| pane.pane_id.clone());
        self.stage.layout_signature = transcript_rows::fingerprint(
            &(
                &tree,
                self.window.width.to_bits(),
                self.window.height.to_bits(),
                self.rail_open,
            ),
            0,
        );
        let sizing = self.state.sizing_of(&workspace_id);
        let tiles = self.tile(&tree, &sizing, focused.as_deref(), &[], palette, cx);
        self.reveal_pending(&tree, &sizing);
        // A pane keeps the size its content fits in, whatever its kind (`pane_floors`, the
        // Mac's layout floors): when the panes need more room than the window has, the stage
        // scrolls, sideways and down, and never shrinks one below its floor. A transcript or a
        // terminal under the pointer keeps its own vertical scrolling.
        //
        // A scroller clips what it holds to its bounds, and a pane's shadow reaches `PANE_HALO`
        // past its card. The scroller therefore reaches that far beyond the margins the panes
        // keep and gives the room back as padding inside, so the outermost panes draw their
        // whole halo on every side (the Mac leaves the same room around its pane grid).
        let halo = neo::PANE_HALO;
        let (needed_width, needed_height) = sizing.minimum(&tree);
        let stage = div()
            .flex()
            .flex_1()
            .min_w(px(needed_width as f32 + 2.0 * halo))
            .min_h(px(needed_height as f32 + 2.0 * halo))
            .p(px(halo))
            .child(tiles);
        // The scroller moves both ways, for panes that cannot give up their size; the wheel
        // goes down only where nothing under the pointer scrolls that way first.
        let scroller = div()
            .id("tiles")
            .flex()
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.stage.scroll)
            .child(stage);
        super::ui::fade_frame(scroller, &self.stage.scroll, palette)
            .min_h_0()
            .m(px(-halo))
            .into_any_element()
    }

    /// Scrolls the stage to the pane that was asked to be shown, once the stage has a size.
    fn reveal_pending(&mut self, tree: &Node, sizing: &Sizing) {
        let bounds = self.stage.scroll.bounds().size;
        let (width, height) = (
            f64::from(f32::from(bounds.width)),
            f64::from(f32::from(bounds.height)),
        );
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let Some(pane_id) = self.stage.reveal.take() else {
            return;
        };
        let halo = f64::from(neo::PANE_HALO);
        let offset = self.stage.scroll.offset();
        let target = |axis: WorkspacePaneLayoutAxis, scrolled: f32, viewport: f64| {
            pane_span(tree, &pane_id, sizing, &axis).map_or(
                f64::from(-scrolled),
                |(start, extent)| {
                    reveal_offset(start + halo, extent, f64::from(-scrolled), viewport)
                },
            )
        };
        let x = target(
            WorkspacePaneLayoutAxis::Horizontal,
            f32::from(offset.x),
            width,
        );
        let y = target(
            WorkspacePaneLayoutAxis::Vertical,
            f32::from(offset.y),
            height,
        );
        self.stage
            .scroll
            .set_offset(point(px(-(x as f32)), px(-(y as f32))));
    }

    fn tile(
        &mut self,
        node: &Node,
        sizing: &Sizing,
        focused: Option<&str>,
        path: &[usize],
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::V0(leaf) => {
                let pane: Option<Pane> = self.state.pane(&leaf.pane_id).cloned();
                match pane {
                    Some(pane) => {
                        let is_focused = focused == Some(pane.pane_id.as_str());
                        self.pane_card(&pane, is_focused, palette, cx)
                    }
                    None => div().flex_1().into_any_element(),
                }
            }
            Node::V1(split) => {
                // Each child has the room its floor needs and takes a share of what is left by
                // weight, as the Mac lays a split out.
                let mut items: Vec<AnyElement> = Vec::new();
                for (index, branch) in split.children.iter().enumerate() {
                    if index > 0 {
                        items.push(self.divider(path, index - 1, &split.axis, cx));
                    }
                    let child_path = [path, &[index]].concat();
                    let child = self.tile(&branch.node, sizing, focused, &child_path, palette, cx);
                    let floor = sizing.minimum(&branch.node);
                    items.push(
                        share(floor, branch.weight, &split.axis)
                            .child(child)
                            .into_any_element(),
                    );
                }
                let layout = div().flex_1().min_w_0().min_h_0().flex();
                let layout = match split.axis {
                    WorkspacePaneLayoutAxis::Horizontal => layout.flex_row(),
                    WorkspacePaneLayoutAxis::Vertical => layout.flex_col(),
                };
                layout.children(items).into_any_element()
            }
        }
    }
}

/// What stands where the panes would be, as the Mac's empty workspace draws it: the icon of the
/// place when it has one, large and light, over a heading of the window's chrome face.
fn empty(palette: &Palette, glyph: Option<Icon>, message: &str) -> AnyElement {
    empty_state(palette, glyph, message.to_owned(), None).into_any_element()
}
