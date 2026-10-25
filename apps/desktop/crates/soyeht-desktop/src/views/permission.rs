//! The permission card of an agent pane, as the Mac draws it: the request in a flat card with the
//! exact text the person approves (the command, a tool's arguments, the file changes) in wells,
//! Allow as the primary pill, other approvals as secondary pills and rejections in danger text.

use super::{
    agent::pill,
    ui::{Typeface, column, icon, id, row},
};
use crate::{
    assets::Icon,
    theme::{Palette, desktop, pane_interior, space},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px, rgb,
};
use soyeht_client::permission_preview::{self, Kind};
use soyeht_protocol::{
    PermissionFileChange, PermissionOptionKind, PermissionPresentation, PermissionRequest,
};

pub(super) fn card(
    pane_id: &str,
    request: &PermissionRequest,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let buttons: Vec<AnyElement> = request
        .options
        .iter()
        .map(|option| {
            let (pane_id, request_id, option_id) = (
                pane_id.to_owned(),
                request.request_id.clone(),
                option.option_id.clone(),
            );
            let (fill, ink) = match option.kind {
                PermissionOptionKind::AllowOnce => (palette.ink, palette.face),
                PermissionOptionKind::AllowAlways => (palette.well, palette.ink),
                PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways => {
                    (palette.well, palette.danger())
                }
            };
            pill(id("permission", &option.option_id), fill, ink)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.answer_permission(&pane_id, &request_id, &option_id)
                }))
                .child(SharedString::from(option.name.clone()))
                .into_any_element()
        })
        .collect();
    column()
        .gap(px(pane_interior::ROW_SPACING))
        .p(px(pane_interior::CARD_PADDING))
        .rounded(px(pane_interior::CARD_RADIUS))
        .bg(rgb(palette.canvas))
        .child(
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_size(px(pane_interior::HEADING_POINT_SIZE))
                .child(SharedString::from(request.title.clone())),
        )
        .children((!request.detail.is_empty()).then(|| {
            div()
                .text_size(px(pane_interior::BODY_POINT_SIZE))
                .text_color(rgb(palette.secondary))
                .child(SharedString::from(request.detail.clone()))
        }))
        .children(
            request
                .presentation
                .as_ref()
                .map(|presentation| body(presentation, palette)),
        )
        .child(
            row()
                .gap(px(pane_interior::COMPACT_SPACING))
                .children(buttons),
        )
        .into_any_element()
}

/// The typed body of a request: the command, the details of how it runs, a tool's arguments and
/// the file changes, one under the other.
fn body(presentation: &PermissionPresentation, palette: &Palette) -> AnyElement {
    column()
        .gap(px(pane_interior::COMPACT_SPACING))
        .children(
            presentation
                .command
                .as_deref()
                .map(|command| text_box("Command", command, palette)),
        )
        .children(
            presentation
                .execution_details
                .iter()
                .flatten()
                .map(|detail| mono_line(detail, palette.secondary)),
        )
        .children(
            presentation
                .tool_arguments
                .iter()
                .flatten()
                .map(|argument| text_box(&argument.name, &argument.value, palette)),
        )
        .children(
            presentation
                .target_path
                .as_deref()
                .filter(|_| presentation.file_changes.is_empty())
                .map(|path| path_line(path, palette)),
        )
        .children(
            presentation
                .file_changes
                .iter()
                .map(|change| file_change(change, palette)),
        )
        .children(presentation.preview_omitted.then(|| omitted(palette)))
        .into_any_element()
}

/// A line of mono text in a color of the palette.
fn mono_line(text: &str, color: u32) -> AnyElement {
    div()
        .mono()
        .text_size(px(pane_interior::MONO_SMALL_POINT_SIZE))
        .text_color(rgb(color))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

/// A path with the icon of a document before it.
fn path_line(path: &str, palette: &Palette) -> AnyElement {
    row()
        .gap(px(space::X6))
        .child(icon(
            Icon::FileText,
            desktop::INLINE_GLYPH,
            palette.secondary,
        ))
        .child(mono_line(path, palette.secondary))
        .into_any_element()
}

fn omitted(palette: &Palette) -> AnyElement {
    div()
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(palette.warning()))
        .child("Diff preview unavailable; inspect the file before allowing.")
        .into_any_element()
}

/// Exact text the person approves, such as a command or one tool argument: a label over a well
/// that shows up to twelve lines and scrolls past them, sideways too, as the text is never wrapped.
fn text_box(label: &str, text: &str, palette: &Palette) -> AnyElement {
    column()
        .gap(px(space::X6))
        .child(
            div()
                .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                .text_color(rgb(palette.secondary))
                .child(SharedString::from(label.to_owned())),
        )
        .child(well(
            id("permission-text", label),
            permission_preview::line_count(text),
            div()
                .whitespace_nowrap()
                .child(SharedString::from(text.to_owned())),
            palette,
        ))
        .into_any_element()
}

/// The path of a changed file and what changes in it, line by line.
fn file_change(change: &PermissionFileChange, palette: &Palette) -> AnyElement {
    let preview = permission_preview::preview(change);
    column()
        .gap(px(space::X6))
        .child(path_line(&change.path, palette))
        .child(if preview.unavailable {
            omitted(palette)
        } else {
            well(
                id("permission-diff", &change.path),
                preview.rows.len(),
                column()
                    .children(preview.rows.iter().map(|row| {
                        let (mark, color) = match row.kind {
                            Kind::Removed => ("\u{2212}", palette.danger()),
                            Kind::Added => ("+", palette.success()),
                        };
                        div()
                            .text_color(rgb(color))
                            .whitespace_nowrap()
                            .child(SharedString::from(format!("{mark} {}", row.text)))
                    }))
                    .w_full(),
                palette,
            )
        })
        .into_any_element()
}

/// A mono well tall enough for `lines` of its text, at most `VISIBLE_LINE_LIMIT`, scrolling past.
fn well(
    key: SharedString,
    lines: usize,
    content: impl IntoElement,
    palette: &Palette,
) -> AnyElement {
    let line = pane_interior::MONO_SMALL_POINT_SIZE * desktop::LINE_HEIGHT_MONO;
    div()
        .id(key)
        .w_full()
        .max_h(px(permission_preview::visible_lines(lines) as f32 * line
            + 2.0 * pane_interior::COMPACT_SPACING))
        .p(px(pane_interior::COMPACT_SPACING))
        .rounded(px(pane_interior::WELL_RADIUS))
        .bg(rgb(palette.well))
        .overflow_scroll()
        .on_scroll_wheel(super::ui::keep_every_wheel)
        .mono()
        .text_size(px(pane_interior::MONO_SMALL_POINT_SIZE))
        .child(content)
        .into_any_element()
}
