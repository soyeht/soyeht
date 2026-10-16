//! An editor pane, as the Mac draws it: a header with the file's path, the tab strip, the project
//! panel beside the lines, the find bar, the lines with their gutter, and a status bar. The card
//! is the only raised surface; everything inside is flat.

use super::{
    prelude::*,
    switch_popover::well_row,
    ui::{acting, empty_state, header_bar, icon_button, mono_label, owned, pane_card_shell},
};
use crate::{
    editor_view,
    theme::{Palette, desktop, editor, glyph, radius, space, type_scale},
};
use gpui::MouseButton;
use soyeht_client::editor::{EditorFile, page::CRLF};
use soyeht_protocol::{EditorLanguage, Pane};

/// The language as the status bar names it.
fn language_name(file: &EditorFile) -> &'static str {
    match file.doc.language {
        EditorLanguage::PlainText => "Plain Text",
        EditorLanguage::Rust => "Rust",
        EditorLanguage::Swift => "Swift",
        EditorLanguage::Typescript => "Typescript",
        EditorLanguage::Python => "Python",
        EditorLanguage::Json => "Json",
        EditorLanguage::Markdown => "Markdown",
        EditorLanguage::Shell => "Shell",
    }
}

/// `Ctrl+Shift+E`, or `⇧⌘E` on the Mac: the key of the project panel.
fn panel_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{21e7}\u{2318}E"
    } else {
        "Ctrl+Shift+E"
    }
}

/// The path as the header writes it: its folders and file, with a chevron between.
pub(super) fn path_label(path: &str) -> String {
    path.split('/').collect::<Vec<_>>().join(" \u{203a} ")
}

impl Workbench {
    pub(super) fn editor_header(
        &mut self,
        cx: &mut Context<Self>,
        pane: &Pane,
        palette: &'static Palette,
    ) -> Div {
        let file = self.state.editors.file_of_pane(&pane.pane_id);
        let label = file.map_or_else(|| "Editor".to_owned(), |file| path_label(&file.doc.path));
        let (status, color) = match file.map(|file| &file.doc) {
            None => ("\u{b7} loading", palette.muted),
            Some(doc) if doc.conflicted => ("\u{b7} conflict", palette.danger()),
            Some(doc) if doc.dirty => ("\u{b7} unsaved", palette.warning()),
            Some(_) => ("\u{b7} saved", palette.muted),
        };
        let shown = !self.project_panel_hidden.contains(&pane.workspace_id);
        let tip = format!(
            "{} Project Panel ({})",
            if shown { "Hide" } else { "Show" },
            panel_shortcut()
        );
        header_bar(editor::HEADER_HEIGHT)
            .child(
                icon_button(
                    id("editor-panel-toggle", &pane.pane_id),
                    Icon::PanelLeft,
                    if shown {
                        palette.accent
                    } else {
                        palette.secondary
                    },
                    tip,
                    palette,
                )
                .on_click(acting(
                    cx,
                    &pane.pane_id,
                    Workbench::toggle_project_panel,
                )),
            )
            .child(icon(Icon::FileText, desktop::INLINE_GLYPH, palette.ink))
            .child(
                mono_label()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .text_color(rgb(palette.ink))
                    .child(SharedString::from(label)),
            )
            .child(spacer())
            .child(
                div()
                    .flex_none()
                    .text_size(px(type_scale::SMALL))
                    .text_color(rgb(color))
                    .child(status),
            )
    }

    /// The body of an editor pane.
    pub(super) fn editor_body(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        pane: &Pane,
        focused: bool,
    ) -> AnyElement {
        let workspace = pane.workspace_id.clone();
        let sync = self.state.editors.trees.sync(&workspace);
        self.run_effects(sync);
        self.reveal_active_file(pane);
        let file = self.state.editors.file_of_pane(&pane.pane_id);
        let conflicted = file.is_some_and(|file| file.doc.conflicted);
        let loaded = file.is_some_and(|file| file.page.is_some());
        let status = file.map(|file| {
            let head = &file.selection.head;
            [
                language_name(file).to_owned(),
                if file.line_ending() == CRLF {
                    "CRLF"
                } else {
                    "LF"
                }
                .to_owned(),
                format!("Ln {}, Col {}", head.line + 1, head.column + 1),
            ]
        });
        let hidden = self.project_panel_hidden.contains(&workspace);
        let tabs = self.editor_tabs(cx, palette, pane);
        let finding = self.find_bar(pane, cx, palette);
        let panel =
            (!hidden).then(|| self.project_panel(cx, palette, Some(&pane.pane_id), &workspace));
        let pane_id = pane.pane_id.clone();
        let text = if loaded {
            self.editor_text(cx, palette, &pane_id, focused)
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(type_scale::CAPTION))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.secondary))
                .child("Loading editor viewport")
                .into_any_element()
        };
        face_row(palette)
            .children(panel)
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(tabs)
                    .children(conflicted.then(|| conflict_banner(palette)))
                    .children(finding)
                    .child(text)
                    .child(status_bar(status, palette)),
            )
            .into_any_element()
    }

    /// The lines, with the mouse and the wheel wired to the pane.
    fn editor_text(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        pane_id: &str,
        focused: bool,
    ) -> AnyElement {
        div()
            .id(id("editor-text", pane_id))
            .flex_1()
            .min_h_0()
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                owned(cx, pane_id, Workbench::editor_mouse_down),
            )
            .on_mouse_move(owned(cx, pane_id, Workbench::editor_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                owned(cx, pane_id, Workbench::editor_mouse_up),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                owned(cx, pane_id, Workbench::editor_mouse_up),
            )
            .on_scroll_wheel(owned(cx, pane_id, Workbench::editor_scroll))
            .child(editor_view::surface(
                pane_id.to_owned(),
                focused,
                cx.entity(),
                palette,
            ))
            .into_any_element()
    }

    /// The project panel shows the file of the active tab: when that file changes, the folders
    /// above it open and it is selected.
    fn reveal_active_file(&mut self, pane: &Pane) {
        let Some(path) = self
            .state
            .editors
            .file_of_pane(&pane.pane_id)
            .map(|file| file.doc.path.clone())
        else {
            return;
        };
        let ui = &mut self.panes.of(&pane.pane_id).editor;
        if ui.revealed.as_deref() != Some(path.as_str()) {
            ui.revealed = Some(path.clone());
            let effects = self.state.editors.trees.reveal(&pane.workspace_id, &path);
            self.run_effects(effects);
        }
    }

    /// A workspace's editor place with no file open: the project panel to pick one from.
    pub(super) fn editor_empty(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        workspace_id: &str,
    ) -> AnyElement {
        let sync = self.state.editors.trees.sync(workspace_id);
        self.run_effects(sync);
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let panel = self.project_panel(cx, palette, None, workspace_id);
        let detail = if connected {
            "Pick one in Files."
        } else {
            "Waiting for the engine."
        };
        let message = empty_state(
            palette,
            Some(Icon::FileText),
            "Open a file to begin",
            Some(detail.into()),
        );
        pane_card_shell(palette)
            .child(face_row(palette).child(panel).child(message))
            .into_any_element()
    }
}

/// The face the panel and the text of an editor sit on, in a row.
fn face_row(palette: &Palette) -> Div {
    row()
        .items_stretch()
        .flex_1()
        .min_h_0()
        .bg(rgb(palette.face))
        .rounded(px(radius::CONTROL))
        .overflow_hidden()
}

/// The line under the tabs when the file changed on disk while it had edits of its own.
fn conflict_banner(palette: &Palette) -> AnyElement {
    well_row(palette)
        .flex()
        .flex_row()
        .items_center()
        .flex_none()
        .gap(px(space::X8))
        .mx(px(space::X10))
        .mb(px(space::X6))
        .py(px(space::X8))
        .text_color(rgb(palette.ink))
        .child(icon(Icon::Alert, glyph::CHEVRON, palette.danger()))
        .child("The file changed on disk while this draft was open.")
        .into_any_element()
}

/// The strip under the lines: the language, the encoding, the line ending and the caret.
fn status_bar(status: Option<[String; 3]>, palette: &Palette) -> AnyElement {
    let [language, ending, place] = status.unwrap_or_default();
    row()
        .h(px(editor::STATUS_HEIGHT))
        .flex_none()
        .px(px(space::X12))
        .gap(px(space::X14))
        .bg(rgb(palette.canvas))
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.secondary))
        .child(language)
        .child("UTF-8")
        .child(ending)
        .child(place)
        .into_any_element()
}
