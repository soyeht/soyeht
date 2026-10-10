//! The neumorphic surfaces of the window chrome: the raised card, pill and button, the inset
//! well, and the pane shell. Only chrome and the pane shell are raised or inset; everything
//! inside a pane is flat.

use crate::theme::{Palette, elevation::Elevation, pane_shell};
use gpui::{BoxShadow, Div, Styled, div, point, px, rgb};

fn shadow(color: u32, offset: f32, blur: f32, inset: bool) -> BoxShadow {
    BoxShadow {
        color: rgb(color).into(),
        offset: point(px(offset), px(offset)),
        blur_radius: px(blur),
        spread_radius: px(0.0),
        inset,
    }
}

/// A `face` surface lifted off the canvas: `shadow_light` up and left, `shadow_dark` down and
/// right (`neoRaised`).
pub fn raised(palette: &Palette, radius: f32, elevation: Elevation) -> Div {
    div().bg(rgb(palette.face)).rounded(px(radius)).shadow(vec![
        shadow(
            palette.shadow_light,
            -elevation.light_offset,
            elevation.light_blur,
            false,
        ),
        shadow(
            palette.shadow_dark,
            elevation.dark_offset,
            elevation.dark_blur,
            false,
        ),
    ])
}

/// A `well` surface pressed into the canvas: the selected tab (`neoWell`). The design tool's
/// blur is twice the depth.
pub fn well(palette: &Palette, radius: f32, depth: f32) -> Div {
    let surface = div().bg(rgb(palette.well)).rounded(px(radius)).shadow(vec![
        shadow(palette.well_shadow, depth, depth * 2.0, true),
        shadow(palette.well_rim, -depth, depth * 2.0, true),
    ]);
    match palette.well_lip {
        Some(lip) => surface.border_1().border_color(rgb(lip)),
        None => surface,
    }
}

/// The pane shell of the approved frames: the only raised surface a pane draws.
pub fn pane_shell(palette: &Palette) -> Div {
    raised(
        palette,
        pane_shell::CORNER_RADIUS,
        Elevation {
            light_offset: pane_shell::SHADOW_OFFSET,
            light_blur: pane_shell::SHADOW_BLUR,
            dark_offset: pane_shell::SHADOW_OFFSET,
            dark_blur: pane_shell::SHADOW_BLUR,
        },
    )
}
