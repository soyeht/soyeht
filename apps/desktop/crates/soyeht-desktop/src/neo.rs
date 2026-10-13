//! The neumorphic surfaces of the window chrome: the raised card, pill and button, the inset
//! well, and the pane shell. Only chrome and the pane shell are raised or inset; everything
//! inside a pane is flat.

use crate::theme::{Palette, desktop, elevation::Elevation, pane_shell};
use gpui::{BoxShadow, Div, Hsla, Styled, div, point, px, rgb};

fn inset_shadow(color: u32, offset: f32, blur: f32) -> BoxShadow {
    BoxShadow {
        color: rgb(color).into(),
        offset: point(px(offset), px(offset)),
        blur_radius: px(blur / 2.0),
        spread_radius: px(0.0),
        inset: true,
    }
}

/// A drop shadow of GPUI: `blur_radius` is the standard deviation of a Gaussian, half the blur
/// the design tokens give (the design tool's blur).
fn drop_shadow(color: u32, opacity: f32, offset: f32, sigma: f32) -> BoxShadow {
    BoxShadow {
        color: Hsla::from(rgb(color)).opacity(opacity),
        offset: point(px(offset), px(offset)),
        blur_radius: px(sigma),
        spread_radius: px(0.0),
        inset: false,
    }
}

/// A `face` surface lifted off the canvas: `shadow_light` up and left, `shadow_dark` down and
/// right (`neoRaised`).
///
/// The Mac stacks `.shadow(light).shadow(dark)`, so its dark shadow is also cast by the light
/// halo, over it, and SwiftUI blurs a little narrower than the radius it is given. Three GPUI
/// shadows stand in for it: the face's dark shadow, the light one, and the halo's dark shadow
/// over the light. The scales (`desktop::SHADOW_*`) were fitted against the Mac's renders of every
/// elevation on five face sizes, for the light palette; they hold, with half the error of a plain
/// offset pair, for the dark ones.
pub fn raised(palette: &Palette, radius: f32, elevation: Elevation) -> Div {
    let light = elevation.light_blur / 2.0;
    let dark = elevation.dark_blur / 2.0;
    let halo = light.hypot(dark);
    div().bg(rgb(palette.face)).rounded(px(radius)).shadow(vec![
        drop_shadow(
            palette.shadow_dark,
            desktop::SHADOW_DARK_OPACITY,
            elevation.dark_offset * desktop::SHADOW_DARK_OFFSET_SCALE,
            dark * desktop::SHADOW_DARK_SIGMA_SCALE,
        ),
        drop_shadow(
            palette.shadow_light,
            1.0,
            -elevation.light_offset * desktop::SHADOW_LIGHT_OFFSET_SCALE,
            light * desktop::SHADOW_LIGHT_SIGMA_SCALE,
        ),
        drop_shadow(
            palette.shadow_dark,
            desktop::SHADOW_HALO_OPACITY,
            halo * desktop::SHADOW_HALO_OFFSET_SCALE,
            halo * desktop::SHADOW_HALO_SIGMA_SCALE,
        ),
    ])
}

/// A `well` surface pressed into the canvas: the selected tab (`neoWell`). The design tool's
/// blur is twice the depth.
pub fn well(palette: &Palette, radius: f32, depth: f32) -> Div {
    let surface = div().bg(rgb(palette.well)).rounded(px(radius)).shadow(vec![
        inset_shadow(palette.well_shadow, depth, depth * 2.0),
        inset_shadow(palette.well_rim, -depth, depth * 2.0),
    ]);
    match palette.well_lip {
        Some(lip) => surface.border_1().border_color(rgb(lip)),
        None => surface,
    }
}

/// How far the pane shell's shadows reach past the card (offset plus blur), as the Mac's
/// `PaneShellMetrics.halo`. Nothing around a pane may clip it.
pub const PANE_HALO: f32 = pane_shell::SHADOW_OFFSET + pane_shell::SHADOW_BLUR;

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
