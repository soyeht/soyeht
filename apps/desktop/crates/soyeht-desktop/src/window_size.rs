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

/// The size of the window as the last frame saw it. Before the first frame nothing is measured,
/// and a popover that fits inside the window gets all the room it asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl Default for WindowSize {
    fn default() -> Self {
        Self {
            width: f32::MAX,
            height: f32::MAX,
        }
    }
}

/// The display the window is sized for: the primary one. A Wayland session names none (GPUI has
/// no primary display there, and the window would open at the design size, which a smaller
/// screen holds only by maximizing it, and restoring then gives back a window larger than the
/// screen), so the smallest of the displays stands in: the window fits wherever it opens.
pub fn display_to_fit(primary: Option<(f32, f32)>, all: &[(f32, f32)]) -> Option<(f32, f32)> {
    primary.or_else(|| {
        all.iter()
            .copied()
            .min_by(|a, b| (a.0 * a.1).total_cmp(&(b.0 * b.1)))
    })
}

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
    fn the_primary_display_is_the_one_the_window_is_sized_for() {
        let all = [(1920.0, 1080.0), (1280.0, 800.0)];
        assert_eq!(
            display_to_fit(Some((1920.0, 1080.0)), &all),
            Some((1920.0, 1080.0))
        );
    }

    #[test]
    fn a_session_with_no_primary_display_sizes_the_window_for_the_smallest() {
        // A Wayland session: the window opens at 1280x800 on a 960x600 screen otherwise, and
        // its restored size is then larger than the screen.
        let all = [(1920.0, 1080.0), (960.0, 600.0)];
        assert_eq!(display_to_fit(None, &all), Some((960.0, 600.0)));
        assert_eq!(display_to_fit(None, &[]), None);
        let (width, height) = opening_size(display_to_fit(None, &all));
        assert!(width <= 960.0 && height <= 600.0);
    }

    #[test]
    fn the_window_never_goes_below_the_minimum() {
        assert_eq!(opening_size(Some((300.0, 200.0))), MINIMUM);
    }
}
