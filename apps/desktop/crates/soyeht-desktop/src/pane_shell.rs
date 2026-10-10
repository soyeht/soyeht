//! The pane shell of the approved frames: a `face` card with corner radius 20 and two outer
//! shadows, `shadow_dark` at (5, 5) and `shadow_light` at (-5, -5), both with blur 10. It is the
//! only raised surface a pane draws; everything inside a pane is flat.

use crate::theme::Palette;
use gpui::{BoxShadow, Div, Pixels, Styled, div, point, px, rgb};

pub const CORNER_RADIUS: f32 = 20.0;
const SHADOW_OFFSET: f32 = 5.0;
const SHADOW_BLUR: f32 = 10.0;

/// How far the shell's shadows reach past the card. Nothing around a pane may clip this halo,
/// so panes keep at least this much room between them and from the window edge.
pub const HALO: f32 = SHADOW_OFFSET + SHADOW_BLUR;

pub fn shell(palette: &Palette) -> Div {
    let shadow = |color: u32, direction: f32| BoxShadow {
        color: rgb(color).into(),
        offset: point(px(SHADOW_OFFSET * direction), px(SHADOW_OFFSET * direction)),
        blur_radius: px(SHADOW_BLUR),
        spread_radius: Pixels::ZERO,
        inset: false,
    };
    div()
        .bg(rgb(palette.face))
        .rounded(px(CORNER_RADIUS))
        .shadow(vec![
            shadow(palette.shadow_light, -1.0),
            shadow(palette.shadow_dark, 1.0),
        ])
}
