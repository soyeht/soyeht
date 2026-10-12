//! Attachment chips: removable in the composer, read-only in a sent message. A chip shows the
//! file's thumbnail (or the icon of its kind), its name and its format and size, in a flat well.

use super::ui::{column, icon, row};
use crate::{
    assets::Icon,
    theme::{Palette, desktop, pane_interior},
};
use gpui::{
    AnyElement, Div, FontWeight, ImageFormat, ImageSource, IntoElement, ParentElement,
    SharedString, Styled, StyledImage, div, img, px, rgb,
};
use soyeht_client::attachments::{Chip, Glyph};
use soyeht_protocol::PromptAttachment;
use std::sync::Arc;

fn glyph_icon(glyph: Glyph) -> Icon {
    match glyph {
        Glyph::Image => Icon::Image,
        Glyph::Audio => Icon::Audio,
        Glyph::Text => Icon::FileText,
        Glyph::Pdf => Icon::Pdf,
        Glyph::File => Icon::File,
    }
}

/// The picture the chip leads with: the thumbnail, rounded, or the icon of what the file is.
fn preview(chip: &Chip, palette: &Palette) -> AnyElement {
    match &chip.thumbnail {
        Some(bytes) => {
            let format = if bytes.starts_with(b"\x89PNG") {
                ImageFormat::Png
            } else {
                ImageFormat::Jpeg
            };
            let image = Arc::new(gpui::Image::from_bytes(format, bytes.clone()));
            img(ImageSource::Image(image))
                .size(px(desktop::ATTACHMENT_THUMBNAIL))
                .flex_none()
                .rounded(px(desktop::ATTACHMENT_THUMBNAIL_RADIUS))
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element()
        }
        None => div()
            .size(px(desktop::ATTACHMENT_THUMBNAIL))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(
                glyph_icon(chip.glyph),
                desktop::COMPOSER_GLYPH,
                palette.secondary,
            ))
            .into_any_element(),
    }
}

/// One chip. The composer adds its remove control after it.
pub(super) fn chip_view(chip: &Chip, palette: &Palette) -> Div {
    row()
        .flex_none()
        .gap(px(pane_interior::COMPACT_SPACING))
        .px(px(pane_interior::COMPACT_SPACING))
        .py(px(pane_interior::TIGHT_SPACING))
        .rounded(px(pane_interior::WELL_RADIUS))
        .bg(rgb(palette.well))
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .child(preview(chip, palette))
        .child(
            column()
                .max_w(px(desktop::ATTACHMENT_CHIP_NAME_WIDTH))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.ink))
                        .child(SharedString::from(chip.name.clone())),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(rgb(palette.muted))
                        .child(SharedString::from(chip.detail.clone())),
                ),
        )
}

/// The chips of a sent message, read-only, in a row that wraps.
pub(super) fn sent_chips(sent: &[PromptAttachment], palette: &Palette) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(pane_interior::COMPACT_SPACING))
        .children(
            sent.iter()
                .enumerate()
                .map(|(index, attachment)| chip_view(&Chip::of_sent(attachment, index), palette)),
        )
}
