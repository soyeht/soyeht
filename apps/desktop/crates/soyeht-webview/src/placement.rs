//! Where a page that is a window of its own goes on the screen: the pane's place in the window's
//! client area, in logical pixels, turned into the device pixels of the screen. Pure arithmetic,
//! so the rule is tested on every system.

use crate::{Placement, Rect};

/// The most a side of a page may take, in device pixels: what the system's web view accepts.
const MAX_SIDE: i32 = 16_384;

/// A rectangle in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where the window that holds a page goes, and where the page is inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    /// The part of the screen the page shows in: the pane's area cut to what is on screen.
    pub host: DeviceRect,
    /// The page's whole area in the host's own coordinates, so a pane that is cut shows the
    /// right part of an uncut page.
    pub page: DeviceRect,
    /// False when nothing of the pane is on screen, or the pane is not to show.
    pub shown: bool,
}

fn scale_of(placement: &Placement) -> f64 {
    if placement.scale.is_finite() && placement.scale > 0.0 {
        placement.scale
    } else {
        1.0
    }
}

/// `logical` as device pixels, bounded so that the arithmetic after it cannot overflow.
fn pixels(logical: f64, scale: f64) -> i32 {
    let device = (logical * scale).round();
    if !device.is_finite() {
        return 0;
    }
    let bound = f64::from(MAX_SIDE) * 4.0;
    // The clamp makes the conversion exact.
    #[allow(clippy::cast_possible_truncation)]
    let device = device.clamp(-bound, bound) as i32;
    device
}

fn device(rect: &Rect, scale: f64) -> DeviceRect {
    DeviceRect {
        x: pixels(rect.x, scale),
        y: pixels(rect.y, scale),
        width: pixels(rect.width, scale),
        height: pixels(rect.height, scale),
    }
}

/// `placement` on the screen, given where the window's client area starts there. A page has at
/// least a pixel to lay out for, so a pane that has no room yet still has one.
pub(crate) fn layout(placement: &Placement, client_origin: (i32, i32)) -> Layout {
    let scale = scale_of(placement);
    let pane = device(&placement.page, scale);
    let page_width = pane.width.clamp(1, MAX_SIDE);
    let page_height = pane.height.clamp(1, MAX_SIDE);
    let clip = device(&placement.clip, scale);
    let left = pane.x.max(clip.x);
    let top = pane.y.max(clip.y);
    let right = pane
        .x
        .saturating_add(page_width)
        .min(clip.x.saturating_add(clip.width));
    let bottom = pane
        .y
        .saturating_add(page_height)
        .min(clip.y.saturating_add(clip.height));
    let on_screen = right > left && bottom > top && pane.width > 0 && pane.height > 0;
    let (width, height) = if on_screen {
        (right - left, bottom - top)
    } else {
        (1, 1)
    };
    Layout {
        host: DeviceRect {
            x: client_origin.0.saturating_add(left),
            y: client_origin.1.saturating_add(top),
            width,
            height,
        },
        page: DeviceRect {
            x: pane.x.saturating_sub(left),
            y: pane.y.saturating_sub(top),
            width: page_width,
            height: page_height,
        },
        shown: placement.visible && on_screen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowHandle;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn placement(page: Rect, clip: Rect, scale: f64) -> Placement {
        Placement {
            window: WindowHandle(0),
            page,
            clip,
            scale,
            visible: true,
        }
    }

    fn whole(width: f64, height: f64) -> Rect {
        rect(0.0, 0.0, width, height)
    }

    #[test]
    fn a_pane_in_view_is_placed_from_the_client_area_in_device_pixels() {
        let pane = rect(100.0, 50.0, 400.0, 300.0);
        let found = layout(&placement(pane, whole(1000.0, 800.0), 1.0), (1000, 200));
        assert_eq!(
            found.host,
            DeviceRect {
                x: 1100,
                y: 250,
                width: 400,
                height: 300
            }
        );
        assert_eq!(
            found.page,
            DeviceRect {
                x: 0,
                y: 0,
                width: 400,
                height: 300
            }
        );
        assert!(found.shown);
    }

    #[test]
    fn a_pane_cut_by_its_stage_shows_the_part_that_is_on_screen_of_an_uncut_page() {
        // The stage shows x from 200 on: the pane starts 80 to the left of it.
        let pane = rect(120.0, 60.0, 400.0, 300.0);
        let clip = rect(200.0, 0.0, 600.0, 500.0);
        let found = layout(&placement(pane, clip, 1.0), (0, 0));
        assert_eq!(
            found.host,
            DeviceRect {
                x: 200,
                y: 60,
                width: 320,
                height: 300
            }
        );
        assert_eq!(
            found.page,
            DeviceRect {
                x: -80,
                y: 0,
                width: 400,
                height: 300
            }
        );
        assert!(found.shown);
    }

    #[test]
    fn a_pane_wholly_outside_its_clip_is_not_shown() {
        let pane = rect(0.0, 0.0, 100.0, 100.0);
        let found = layout(
            &placement(pane, rect(300.0, 0.0, 100.0, 100.0), 1.0),
            (0, 0),
        );
        assert!(!found.shown);
        assert_eq!((found.host.width, found.host.height), (1, 1));
        let mut hidden = placement(pane, whole(500.0, 500.0), 1.0);
        hidden.visible = false;
        assert!(!layout(&hidden, (0, 0)).shown);
    }

    #[test]
    fn a_scaled_screen_multiplies_the_pane_and_rounds_to_a_pixel() {
        let pane = rect(10.5, 0.2, 100.3, 80.0);
        let found = layout(&placement(pane, whole(1000.0, 1000.0), 1.5), (0, 0));
        assert_eq!(
            found.host,
            DeviceRect {
                x: 16,
                y: 0,
                width: 150,
                height: 120
            }
        );
    }

    #[test]
    fn a_pane_with_no_room_still_has_a_pixel_to_lay_out_for() {
        let found = layout(
            &placement(rect(0.0, 0.0, 0.0, -5.0), whole(10.0, 10.0), 1.0),
            (0, 0),
        );
        assert_eq!((found.page.width, found.page.height), (1, 1));
        assert!(!found.shown);
    }

    #[test]
    fn a_scale_that_is_not_a_number_is_one_and_a_huge_pane_is_bounded() {
        let found = layout(
            &placement(whole(1.0e9, 10.0), whole(1.0e9, 100.0), f64::NAN),
            (0, 0),
        );
        assert_eq!((found.page.width, found.page.height), (MAX_SIDE, 10));
    }
}
