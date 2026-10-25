//! The Markdown preview beside the lines of a Markdown file, as the Mac draws it
//! (`EditorMarkdownPreview`): a PREVIEW band that says which lines it shows when the page is not
//! the whole file, over the page drawn by the blocks of an agent's reply, in the flat interior.

use super::{
    markdown::{self, Opens},
    prelude::*,
    ui::{caption, column, mono_note, row, section_label},
};
use crate::theme::{Palette, editor, pane_interior, space, type_scale};
use soyeht_client::{
    editor::EditorFile,
    markdown::{Preview, Refusal},
};

impl Workbench {
    /// The preview of the file an editor pane shows, when it is a Markdown file with a page.
    pub(super) fn editor_preview(
        &mut self,
        pane_id: &str,
        palette: &'static Palette,
    ) -> Option<AnyElement> {
        let made = self
            .state
            .editors
            .file_of_pane(pane_id)
            .and_then(EditorFile::markdown_preview)?;
        let body = match &made {
            Ok(preview) => {
                // The cache keeps the parse until the page changes: a frame that moves nothing
                // parses nothing.
                let blocks = self.markdown.preview_blocks(pane_id, &preview.source);
                div()
                    .id(id("editor-preview-scroll", pane_id))
                    .flex_1()
                    .min_h_0()
                    .pane_scroll_y()
                    .p(px(space::X14))
                    .text_size(px(pane_interior::BODY_POINT_SIZE))
                    .child(markdown::reply(
                        &blocks,
                        &format!("{pane_id}-preview"),
                        palette,
                        palette.ink,
                        Opens::Nothing,
                    ))
                    .into_any_element()
            }
            Err(Refusal::TooManyLines | Refusal::TooManyBytes) => caption(palette)
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child("This viewport cannot be previewed.")
                .into_any_element(),
        };
        let partial = match &made {
            Ok(preview) if preview.is_partial() => Some(preview),
            _ => None,
        };
        Some(
            column()
                .id(id("editor-preview", pane_id))
                // It starts at its ideal width and gives way to the lines down to its narrowest.
                .flex_shrink(1.0)
                .flex_grow(0.0)
                .flex_basis(px(editor::PREVIEW_WIDTH))
                .min_w(px(editor::PREVIEW_MIN_WIDTH))
                .max_w(px(editor::PREVIEW_MAX_WIDTH))
                .child(band(partial, palette))
                .child(body)
                .into_any_element(),
        )
    }
}

/// The `lines` of the editor with the `preview` beside them, set off by a line. The lines keep
/// their floor (`editor_text` gives them `pane_floors::editor_code_floor` as their narrowest) and
/// the preview gives way to them, down to its narrowest.
pub(super) fn beside_lines(
    lines: AnyElement,
    preview: AnyElement,
    palette: &Palette,
) -> AnyElement {
    row()
        .items_stretch()
        .flex_1()
        .min_h_0()
        .child(lines)
        .child(
            div()
                .flex_none()
                .w(px(pane_interior::SEPARATOR_WIDTH))
                .bg(rgb(palette.well)),
        )
        .child(preview)
        .into_any_element()
}

/// The band over the preview: its name, and the lines it shows when the page is part of the file.
fn band(partial: Option<&Preview>, palette: &Palette) -> Div {
    row()
        .h(px(editor::PREVIEW_HEADER_HEIGHT))
        .flex_none()
        .px(px(space::X10))
        .gap(px(space::X6))
        .bg(rgb(palette.canvas))
        .child(section_label(palette, "PREVIEW"))
        .child(spacer())
        .children(partial.map(|preview| {
            mono_note(palette)
                .text_size(px(type_scale::MICRO))
                .child(SharedString::from(preview.range_label()))
        }))
}
