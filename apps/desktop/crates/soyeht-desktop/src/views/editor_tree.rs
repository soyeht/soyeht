//! The project panel: the workspace's folder as a tree that lists a folder when it is opened. A
//! single click on a file previews it, a double click keeps its tab; the panel's own keys
//! (arrows, Return, Space) are the editor's (`editor_input`).

use super::{
    prelude::*,
    ui::{acting, hint, icon_button, owned, section_label},
};
use crate::theme::{Palette, desktop, editor, radius, space};
use gpui::MouseButton;
use soyeht_client::editor::tree::Row;

/// Rows drawn beyond the ones in view, so that a wheel turn does not show a gap.
const OVERSCAN: usize = 6;

/// What one row of the panel is drawn from.
struct Entry<'a> {
    workspace: &'a str,
    row: &'a Row,
    chosen: bool,
    current: bool,
}

impl Workbench {
    /// The panel of `workspace_id`; `pane_id` is the editor it sits in, when it sits in one.
    pub(super) fn project_panel(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        pane_id: Option<&str>,
        workspace_id: &str,
    ) -> AnyElement {
        let trees = &self.state.editors.trees;
        let rows = trees.rows(workspace_id);
        let selected = trees.selection(workspace_id).map(str::to_owned);
        let shows_ignored = trees.show_ignored(workspace_id);
        let active = pane_id
            .and_then(|pane| self.state.editors.file_of_pane(pane))
            .map(|file| file.doc.path.clone());
        let scroll = match pane_id {
            Some(pane) => self.panes.of(pane).editor.tree_scroll.clone(),
            None => self
                .empty_tree_scroll
                .entry(workspace_id.to_owned())
                .or_default()
                .clone(),
        };
        // Only the rows in view are built, between two spacers that keep the scroll the size of
        // the whole tree.
        let height = f32::from(scroll.bounds().size.height);
        let row_height = desktop::EDITOR_PROJECT_ROW_HEIGHT;
        let top = (-f32::from(scroll.offset().y)).max(0.0);
        let view = if height > 0.0 {
            height
        } else {
            self.window_height
        };
        let first = ((top / row_height) as usize).saturating_sub(OVERSCAN);
        let last = (((top + view) / row_height).ceil() as usize + OVERSCAN).min(rows.len());
        let shown: Vec<AnyElement> = rows
            .get(first..last)
            .unwrap_or_default()
            .iter()
            .map(|row| {
                let entry = Entry {
                    workspace: workspace_id,
                    row,
                    chosen: selected.as_deref() == Some(row.node.path.as_str()),
                    current: active.as_deref() == Some(row.node.path.as_str()),
                };
                self.tree_row(&entry, palette, cx)
            })
            .collect();
        let owner = pane_id.unwrap_or(workspace_id);
        let eye = if shows_ignored {
            Icon::Eye
        } else {
            Icon::EyeOff
        };
        let eye_tip = if shows_ignored {
            "Hide files ignored by Git"
        } else {
            "Show files ignored by Git"
        };
        let header = row()
            .h(px(row_height))
            .px(px(space::X6))
            .gap(px(space::X6))
            .child(section_label(palette, "FILES").flex_1())
            .child(
                icon_button(
                    id("editor-ignored", workspace_id),
                    eye,
                    palette.muted,
                    eye_tip,
                    palette,
                )
                .on_click(acting(cx, workspace_id, Workbench::toggle_ignored)),
            )
            .child(
                icon_button(
                    id("editor-reveal", workspace_id),
                    Icon::Compass,
                    palette.muted,
                    "Reveal the active file",
                    palette,
                )
                .on_click(acting(cx, owner, Workbench::reveal_in_tree)),
            );
        let list = div()
            .id(id("editor-files", owner))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .on_mouse_down(
                MouseButton::Left,
                acting(cx, owner, Workbench::tree_pressed),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().h(px(first as f32 * row_height)))
                    .children(shown)
                    .child(div().h(px(rows.len().saturating_sub(last) as f32 * row_height))),
            );
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(editor::PROJECT_PANEL_WIDTH))
            .h_full()
            .gap(px(space::X2))
            .py(px(space::X10))
            .px(px(space::X8))
            .bg(rgb(palette.canvas))
            .child(header)
            .child(list)
            .into_any_element()
    }

    fn tree_row(
        &mut self,
        entry: &Entry,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let node = &entry.row.node;
        let color = if entry.current {
            palette.accent
        } else if node.ignored {
            palette.muted
        } else {
            palette.ink
        };
        let key = format!("{}/{}", entry.workspace, node.path);
        let tip = if node.ignored {
            format!("{} \u{b7} ignored by Git", node.path)
        } else {
            node.path.clone()
        };
        let chevron = if entry.row.expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let kind = if node.is_directory {
            Icon::Folder
        } else {
            Icon::FileText
        };
        let indent = space::X6 + entry.row.depth as f32 * desktop::EDITOR_PROJECT_INDENT;
        row()
            .id(id("editor-file", &key))
            .h(px(desktop::EDITOR_PROJECT_ROW_HEIGHT))
            .flex_none()
            .pl(px(indent))
            .pr(px(space::X6))
            .gap(px(space::X4))
            .rounded(px(radius::COMPACT))
            .cursor_pointer()
            .when(entry.chosen, |row| row.bg(rgb(palette.well)))
            .when(!entry.chosen, |row| {
                row.hover(|style| style.bg(rgb(palette.hover)))
            })
            .tooltip(hint(tip, palette))
            .on_click(owned(cx, &node.path, Workbench::tree_clicked))
            .child(
                div()
                    .flex_none()
                    .w(px(desktop::EDITOR_PROJECT_INDENT))
                    .children(
                        node.is_directory
                            .then(|| icon(chevron, desktop::CHEVRON_GLYPH, palette.muted)),
                    ),
            )
            .child(icon(
                kind,
                desktop::INLINE_GLYPH,
                if entry.current {
                    palette.accent
                } else {
                    palette.secondary
                },
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .mono()
                    .text_size(px(editor::FONT_POINT_SIZE))
                    .when(entry.current, |name| name.font_weight(FontWeight::BOLD))
                    .text_color(rgb(color))
                    .child(SharedString::from(node.name.clone())),
            )
            .into_any_element()
    }
}
