//! An editor pane, as the Mac draws it: a header with the file's path, the tab strip, the project
//! panel beside the lines, the find bar, the lines with their gutter, and a status bar. The card
//! is the only raised surface; everything inside is flat.

use super::{
    cards::{Role, button},
    editor_preview::beside_lines,
    pane::Action,
    prelude::*,
    switch_popover::well_row,
    ui::{
        acting, bounds_probe, empty_state, header_bar, icon_button, mono_label, owned,
        pane_card_shell,
    },
};
use crate::{
    editor_view, pane_floors, shortcuts,
    theme::{Palette, desktop, editor, glyph, radius, space, type_scale},
};
use gpui::{Entity, MouseButton};
use soyeht_client::editor::{EditorFile, PaneLoad, page::CRLF, refusal::Refusal};
use soyeht_protocol::{EditorLanguage, Pane, client_policy::EDITOR_SAVE_AGAIN};

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

/// What a header says of the file: whether it is saved, and the color of the word. A pane that
/// holds a file but no page, whose read failed, says what its body says.
pub(super) fn file_status(
    file: Option<&EditorFile>,
    failed: bool,
    palette: &Palette,
) -> (&'static str, u32) {
    match file {
        _ if failed => ("\u{b7} failed", palette.danger()),
        None => ("\u{b7} loading", palette.muted),
        Some(file) if file.doc.conflicted => ("\u{b7} conflict", palette.danger()),
        // The caret is on its way to a page the engine has not sent: keys wait for it.
        Some(file) if file.is_landing() => ("\u{b7} moving", palette.muted),
        // The engine could not save it: the pane says why under the tabs.
        Some(file) if file.save_status.failure().is_some() => {
            ("\u{b7} not saved", palette.danger())
        }
        Some(file) if file.doc.dirty => ("\u{b7} unsaved", palette.warning()),
        Some(_) => ("\u{b7} saved", palette.muted),
    }
}

/// The path as the header writes it: its folders and file, with a chevron between.
pub(super) fn path_label(path: &str) -> String {
    path.split('/').collect::<Vec<_>>().join(" \u{203a} ")
}

/// Reports how wide the room beside the project panel is, which decides whether the Markdown
/// preview fits (`pane_floors::preview_fits`). It takes no room of its own.
fn area_probe(workbench: Entity<Workbench>, pane_id: String) -> impl IntoElement {
    bounds_probe(move |bounds, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.editor_area_measured(&pane_id, bounds.size.width, cx);
        });
    })
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
        let status = self.file_state_word(&pane.pane_id, palette);
        let shown = !self.state.project_panel_hidden(&pane.workspace_id);
        let tip = format!(
            "{} Project Panel ({})",
            if shown { "Hide" } else { "Show" },
            shortcuts::PROJECT_PANEL.label()
        );
        let close = self.header_button(pane, Action::Close, palette, cx);
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
            .child(status)
            .child(close)
    }

    /// What a header says of the file of a pane: whether it is saved, in the word's color.
    pub(super) fn file_state_word(&self, pane_id: &str, palette: &Palette) -> Div {
        let failed = matches!(self.state.editors.pane_load(pane_id), PaneLoad::Failed(_));
        let file = self.state.editors.file_of_pane(pane_id);
        let (word, color) = file_status(file, failed, palette);
        div()
            .flex_none()
            .text_size(px(type_scale::SMALL))
            .text_color(rgb(color))
            .child(word)
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
        let save_failure = file.and_then(|file| file.save_status.text());
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
        let notice = file
            .map(|file| file.dropped_keys)
            .filter(|dropped| *dropped > 0)
            .map(|dropped| format!("{dropped} keys could not be applied"));
        let hidden = self.state.project_panel_hidden(&workspace);
        let tabs = self.editor_tabs(cx, palette, pane);
        let finding = self.find_bar(pane, cx, palette);
        let panel =
            (!hidden).then(|| self.project_panel(cx, palette, Some(&pane.pane_id), &workspace));
        let pane_id = pane.pane_id.clone();
        let refused = self.state.editors.refusal(&workspace).is_some();
        let text = if let Some(refusal) = self.state.editors.refusal(&workspace) {
            refusal_state(refusal, palette).into_any_element()
        } else if loaded {
            self.editor_text(cx, palette, &pane_id, focused)
        } else {
            match self.state.editors.pane_load(&pane_id) {
                PaneLoad::Failed(reason) => Self::load_failed(reason, &pane_id, palette, cx),
                PaneLoad::Loading | PaneLoad::Ready => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(type_scale::CAPTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.secondary))
                    .child("Loading editor viewport")
                    .into_any_element(),
            }
        };
        // A Markdown file has its preview beside the lines, set off by a line, when the room
        // holds both; a file the engine refused as text has none, as on the Mac.
        let fits = pane_floors::preview_fits(self.panes.of(&pane_id).editor.area_width);
        let preview = (loaded && !refused && fits)
            .then(|| self.editor_preview(&pane_id, palette))
            .flatten();
        let text = match preview {
            Some(preview) => beside_lines(text, preview, palette),
            None => text,
        };
        face_row(palette)
            .children(panel)
            .child(
                column()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(area_probe(cx.entity(), pane_id.clone()))
                    .child(tabs)
                    .children(conflicted.then(|| conflict_banner(palette)))
                    .children(
                        save_failure.map(|text| save_failure_banner(text, &pane_id, palette, cx)),
                    )
                    .children(finding)
                    .child(text)
                    .child(status_bar(status, notice, palette)),
            )
            .into_any_element()
    }

    /// What a pane shows when its file could not be read: the engine's reason, and a button that
    /// asks again.
    pub(super) fn load_failed(
        reason: String,
        pane_id: &str,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        empty_state(
            palette,
            Some(Icon::Alert),
            "This file could not be loaded",
            Some(SharedString::from(reason)),
        )
        .child(
            button(
                id("editor-retry", pane_id),
                Role::Secondary,
                "Retry",
                palette,
            )
            .own_press()
            .on_click(acting(cx, pane_id, Workbench::retry_editor_load)),
        )
        .into_any_element()
    }

    /// Asks again for the first page of a pane that could not get one.
    pub(super) fn retry_editor_load(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.editor_retry(pane_id);
        self.run_effects(effects);
        cx.notify();
    }

    /// The lines, with the mouse and the wheel wired to the pane.
    fn editor_text(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
        pane_id: &str,
        focused: bool,
    ) -> AnyElement {
        // The lines never get narrower than their floor: beside the Markdown preview it is the
        // preview that gives way.
        let floor = pane_floors::editor_code_floor(&pane_floors::FontMetrics::reference()) as f32;
        div()
            .id(id("editor-text", pane_id))
            .flex_1()
            .min_w(px(floor))
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
        let message = match self.state.editors.refusal(workspace_id) {
            Some(refusal) => refusal_state(refusal, palette),
            None => empty_state(
                palette,
                Some(Icon::FileText),
                "Open a file to begin",
                Some(detail.into()),
            ),
        };
        pane_card_shell(palette)
            .child(face_row(palette).child(panel).child(message))
            .into_any_element()
    }
}

/// What stands in place of the text of a file the engine refused as text: its path and the
/// reason, with nothing to confirm.
fn refusal_state(refusal: &Refusal, palette: &Palette) -> Div {
    empty_state(
        palette,
        Some(Icon::FileText),
        path_label(&refusal.path),
        Some(refusal.text.into()),
    )
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
pub(super) fn conflict_banner(palette: &Palette) -> AnyElement {
    strip(palette)
        .child(icon(Icon::Alert, glyph::CHEVRON, palette.danger()))
        .child("The file changed on disk while this draft was open.")
        .into_any_element()
}

/// The line under the tabs when the engine could not save the file: why, and a button that
/// saves it again. Closing the tab is the header's.
pub(super) fn save_failure_banner(
    text: &'static str,
    pane_id: &str,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    strip(palette)
        .child(icon(Icon::Alert, glyph::CHEVRON, palette.danger()))
        .child(div().flex_1().min_w_0().child(text))
        .child(
            button(
                id("editor-save-again", pane_id),
                Role::Secondary,
                EDITOR_SAVE_AGAIN,
                palette,
            )
            .own_press()
            .on_click(acting(cx, pane_id, Workbench::save_pane_file)),
        )
        .into_any_element()
}

/// A line of words on a well, under the header of a pane, with room around it.
pub(super) fn strip(palette: &Palette) -> Div {
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
}

/// The strip under the lines: the language, the encoding, the line ending and the caret.
fn status_bar(
    status: Option<[String; 3]>,
    notice: Option<String>,
    palette: &Palette,
) -> AnyElement {
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
        .children(notice.map(|notice| div().text_color(rgb(palette.warning())).child(notice)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeName;
    use soyeht_protocol::{EditorSaveFailure, FileDocument};

    fn file(dirty: bool) -> EditorFile {
        let doc: FileDocument = serde_json::from_value(serde_json::json!({
            "fileId": "f", "paneId": "p", "workspaceId": "w", "path": "a.rs",
            "language": "rust", "revision": "1", "diskRevision": "1", "lineCount": 1,
            "byteCount": "1", "dirty": dirty, "conflicted": false, "watching": true
        }))
        .unwrap();
        EditorFile::new(doc)
    }

    #[test]
    fn a_file_the_engine_could_not_save_says_so_in_the_header_not_just_unsaved() {
        let palette = ThemeName::DEFAULT.palette();
        let mut file = file(true);
        assert_eq!(
            file_status(Some(&file), false, palette),
            ("\u{b7} unsaved", palette.warning())
        );
        file.save_status.told(EditorSaveFailure::Missing);
        assert_eq!(
            file_status(Some(&file), false, palette),
            ("\u{b7} not saved", palette.danger())
        );
        file.save_status.saved();
        assert_eq!(file_status(Some(&file), false, palette).0, "\u{b7} unsaved");
    }
}
