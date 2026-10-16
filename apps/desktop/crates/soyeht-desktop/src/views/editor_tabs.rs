//! The tab strip of the editor: one tab per file, in the engine's order. The active one is a well;
//! one that is a preview is in italics until its file is edited, and an unsaved file shows a dot
//! where the close button appears when the pointer is over the tab.

use super::{
    prelude::*,
    ui::{acting, hint, icon_button, owned, sideways_scroller},
};
use crate::theme::{Palette, desktop, editor, space};
use gpui::MouseButton;
use soyeht_client::editor::tabs::is_preview;
use soyeht_protocol::{EditorTab, Pane};

/// The name of the file at the end of a path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Workbench {
    pub(super) fn editor_tabs(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        pane: &Pane,
    ) -> AnyElement {
        let tabs = self
            .state
            .editors
            .tab_list(&pane.workspace_id, self.state.panes());
        let items: Vec<AnyElement> = tabs
            .iter()
            .map(|tab| self.editor_tab(tab, &pane.pane_id, palette, cx))
            .collect();
        let scroll = self.panes.of(&pane.pane_id).editor.tabs_scroll.clone();
        let strip = row().gap(px(space::X4)).children(items);
        row()
            .h(px(desktop::EDITOR_TAB_BAR_HEIGHT))
            .flex_none()
            .px(px(space::X8))
            .bg(rgb(palette.canvas))
            .child(sideways_scroller(
                id("editor-tabs", &pane.pane_id),
                &scroll,
                strip,
                palette,
            ))
            .into_any_element()
    }

    fn editor_tab(
        &mut self,
        tab: &EditorTab,
        shown: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let file = self.state.editors.file(&tab.file_id).map(|file| &file.doc);
        let active = tab.pane_id == shown;
        let preview = is_preview(tab, file.map(|doc| doc.revision.as_str()));
        let dirty = file.is_some_and(|doc| doc.dirty);
        let group = id("editor-tab-group", &tab.pane_id);
        let color = if active {
            palette.accent
        } else {
            palette.secondary
        };
        let tip = if preview {
            format!("{} \u{b7} preview", tab.path)
        } else {
            tab.path.clone()
        };
        let weight = if active {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        row()
            .id(id("editor-tab", &tab.pane_id))
            .group(group.clone())
            .h(px(editor::TAB_HEIGHT))
            .flex_none()
            .px(px(space::X12))
            .gap(px(space::X6))
            .rounded_full()
            .cursor_pointer()
            .text_color(rgb(color))
            .when(active, |tab| tab.bg(rgb(palette.well)))
            .when(!active, |tab| {
                tab.hover(|style| style.bg(rgb(palette.hover)))
            })
            .tooltip(hint(tip, palette))
            .on_click(owned(cx, &tab.pane_id, Workbench::tab_clicked))
            .on_mouse_down(
                MouseButton::Middle,
                acting(cx, &tab.pane_id, Workbench::close_tab),
            )
            .child(icon(Icon::FileText, desktop::INLINE_GLYPH, color))
            .child(
                div()
                    .mono()
                    .text_size(px(editor::FONT_POINT_SIZE))
                    .font_weight(weight)
                    .when(preview, |name| name.italic())
                    .child(SharedString::from(file_name(&tab.path).to_owned())),
            )
            .child(close_slot(&tab.pane_id, &group, dirty, palette, cx))
            .into_any_element()
    }
}

/// The corner of a tab where an unsaved file shows a dot and the pointer over the tab shows
/// the button that closes it.
fn close_slot(
    pane_id: &str,
    group: &SharedString,
    dirty: bool,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let dot = div()
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .when(dirty, |slot| {
            slot.group_hover(group.clone(), |style| style.opacity(0.0))
                .child(
                    div()
                        .size(px(desktop::EDITOR_DIRTY_DOT))
                        .rounded_full()
                        .bg(rgb(palette.warning())),
                )
        });
    let close = icon_button(
        id("editor-tab-close", pane_id),
        Icon::Close,
        palette.secondary,
        "Close tab",
        palette,
    )
    .absolute()
    .size_full()
    .justify_center()
    .opacity(0.0)
    .group_hover(group.clone(), |style| style.opacity(1.0))
    .on_click(acting(cx, pane_id, Workbench::close_tab));
    div()
        .relative()
        .flex_none()
        .size(px(desktop::EDITOR_TAB_CLOSE_SLOT))
        .child(dot)
        .child(close)
        .into_any_element()
}
