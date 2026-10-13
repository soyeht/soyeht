//! A usage ring of the rail: the provider's logo in a disc, a band-colored arc for the share
//! used, and the percentage under it.

use super::ui::{Typeface, column, logo};
use crate::{
    assets,
    theme::{Palette, usage_ring},
};
use gpui::{
    AnyElement, Hsla, InteractiveElement, IntoElement, ParentElement, PathBuilder, Styled, canvas,
    div, point, px, rgb,
};
use soyeht_client::usage::{Band, Presentation};
use soyeht_protocol::UsageReading;

/// The palette role of a usage band; usage never introduces a color of its own.
pub fn band_color(band: Band, palette: &Palette) -> u32 {
    match band {
        Band::Success => palette.success(),
        Band::Warning => palette.warning(),
        Band::Danger => palette.danger(),
    }
}

pub fn ring(reading: &UsageReading, palette: &Palette) -> AnyElement {
    let shown = Presentation(reading);
    let emphasis = shown.emphasis();
    let arc = shown.fraction().zip(shown.band()).map(|(fraction, band)| {
        let mut color: Hsla = rgb(band_color(band, palette)).into();
        color.a *= emphasis.arc_opacity();
        (fraction, color)
    });
    let name = reading.provider.display_name();
    let diameter = usage_ring::DIAMETER;
    let track = usage_ring::FACE_DIAMETER + usage_ring::TRACK_WIDTH;
    column()
        .id(super::ui::id("usage-ring", name))
        .w(px(diameter))
        .h(px(usage_ring::HEIGHT))
        .flex_none()
        .items_center()
        .opacity(emphasis.ring_opacity())
        .child(
            div()
                .relative()
                .size(px(diameter))
                .flex_none()
                .child(
                    div()
                        .absolute()
                        .top(px((diameter - track) / 2.0))
                        .left(px((diameter - track) / 2.0))
                        .size(px(track))
                        .rounded_full()
                        .bg(rgb(palette.well)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px((diameter - usage_ring::FACE_DIAMETER) / 2.0
                            + usage_ring::TRACK_WIDTH / 2.0))
                        .left(px((diameter - usage_ring::FACE_DIAMETER) / 2.0
                            + usage_ring::TRACK_WIDTH / 2.0))
                        .size(px(usage_ring::FACE_DIAMETER - usage_ring::TRACK_WIDTH))
                        .rounded_full()
                        .bg(rgb(palette.face)),
                )
                .child(ring_canvas(arc, None, diameter, usage_ring::ARC_WIDTH))
                .children(assets::logo_path(name).map(|path| {
                    div()
                        .absolute()
                        .top(px((diameter - usage_ring::GLYPH_SIZE) / 2.0))
                        .left(px((diameter - usage_ring::GLYPH_SIZE) / 2.0))
                        .child(logo(path, usage_ring::GLYPH_SIZE, palette.ink))
                })),
        )
        .child(
            div()
                .h(px(usage_ring::PERCENTAGE_HEIGHT))
                .flex()
                .items_center()
                .chrome()
                .font_weight(gpui::FontWeight::BOLD)
                .text_size(px(usage_ring::PERCENTAGE_SIZE))
                .text_color(rgb(if shown.is_stale() {
                    palette.muted
                } else {
                    palette.secondary
                }))
                .child(shown.percentage()),
        )
        .into_any_element()
}

/// The ring of `diameter`: an optional full-circle `track`, and over it the arc from the top,
/// clockwise, covering `fraction` of the circle.
pub fn ring_canvas(
    arc: Option<(f64, Hsla)>,
    track: Option<Hsla>,
    diameter: f32,
    width: f32,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let radius = diameter / 2.0 - width / 2.0;
            let center = bounds.center();
            let at = |turn: f64| {
                let angle = turn * std::f64::consts::TAU;
                point(
                    center.x + px(radius * angle.sin() as f32),
                    center.y - px(radius * angle.cos() as f32),
                )
            };
            let radii = point(px(radius), px(radius));
            let mut stroke = |fraction: f64, color: Hsla| {
                let mut path = PathBuilder::stroke(px(width));
                path.move_to(at(0.0));
                if fraction >= 0.999 {
                    path.arc_to(radii, px(0.0), false, true, at(0.5));
                    path.arc_to(radii, px(0.0), false, true, at(0.0));
                } else {
                    path.arc_to(radii, px(0.0), fraction > 0.5, true, at(fraction));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            };
            if let Some(track) = track {
                stroke(1.0, track);
            }
            if let Some((fraction, color)) = arc {
                stroke(fraction, color);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
