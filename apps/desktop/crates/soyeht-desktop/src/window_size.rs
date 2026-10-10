//! The size the window opens at: the size it is designed for, never larger than the display it
//! opens on.

use crate::theme::desktop;

/// What the window is designed for.
pub const DESIGN: (f32, f32) = (desktop::WINDOW_WIDTH, desktop::WINDOW_HEIGHT);
/// The smallest size the layout still works at.
pub const MINIMUM: (f32, f32) = (
    desktop::WINDOW_MINIMUM_WIDTH,
    desktop::WINDOW_MINIMUM_HEIGHT,
);
/// Room kept beside the window on a small display, for the frame.
const WIDTH_MARGIN: f32 = desktop::WINDOW_WIDTH_MARGIN;
/// Room kept above and below it, for a taskbar, a dock or a top panel (a Windows taskbar is 48
/// points, and the window is centered on the whole display).
const HEIGHT_MARGIN: f32 = desktop::WINDOW_HEIGHT_MARGIN;

/// `DESIGN`, reduced so that it fits `display` with a margin, but not below `MINIMUM`. With no
/// display to measure the design size is used as it is.
pub fn opening_size(display: Option<(f32, f32)>) -> (f32, f32) {
    let Some((width, height)) = display else {
        return DESIGN;
    };
    (
        DESIGN.0.min(width - WIDTH_MARGIN).max(MINIMUM.0),
        DESIGN.1.min(height - HEIGHT_MARGIN).max(MINIMUM.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_display_gets_the_design_size() {
        assert_eq!(opening_size(Some((2560.0, 1440.0))), DESIGN);
        assert_eq!(opening_size(None), DESIGN);
    }

    #[test]
    fn a_small_display_shrinks_the_window_to_fit_with_a_margin() {
        assert_eq!(opening_size(Some((1024.0, 768.0))), (960.0, 648.0));
        assert_eq!(opening_size(Some((1280.0, 800.0))), (1216.0, 680.0));
    }

    #[test]
    fn the_window_never_goes_below_the_minimum() {
        assert_eq!(opening_size(Some((300.0, 200.0))), MINIMUM);
    }
}
