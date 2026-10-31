//! What a frame draws over the panes outside the popup menus: the tooltip beside the pointer. A
//! page that is a window of its own (Windows, ADR 0124) shows above everything GPUI draws, so an
//! overlay says where it is and a page it overlaps goes off the screen while it shows. A menu or a
//! popover needs no footprint: it hides every page while it is open (`Workbench::web_covered`).
//!
//! A frame prepares its overlays (the tooltip, after the view) before it paints, and the page is
//! placed while the panes paint, so the page reads this frame's footprints, not the last one's.
//! The workbench clears them as its view is built.

use gpui::{App, Bounds, Global, IntoElement, Pixels, Styled, canvas};

/// Where this frame's overlays are, in the window's own coordinates.
#[derive(Default)]
struct Overlays(Vec<Bounds<Pixels>>);

impl Global for Overlays {}

impl Overlays {
    fn covers(&self, page: Bounds<Pixels>) -> bool {
        self.0.iter().any(|overlay| overlay.intersects(&page))
    }
}

/// A new frame begins: nothing is drawn over the panes yet.
pub(crate) fn begin_frame(cx: &mut App) {
    cx.default_global::<Overlays>().0.clear();
}

/// An element that tells where its parent is drawn, to go inside an overlay, which must be
/// `relative` or `absolute`.
pub(crate) fn footprint() -> impl IntoElement {
    // At once, not after the frame as `ui::bounds_probe` reports: the page is placed in this
    // frame's paint and must find the footprint there.
    canvas(
        |bounds, _, cx| cx.default_global::<Overlays>().0.push(bounds),
        |_, (), _, _| {},
    )
    .absolute()
    .size_full()
}

/// Whether an overlay of this frame is over a page placed at `page`.
pub(crate) fn covers(page: Bounds<Pixels>, cx: &App) -> bool {
    cx.try_global::<Overlays>()
        .is_some_and(|overlays| overlays.covers(page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn at(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn a_page_is_covered_only_by_the_overlay_that_overlaps_it() {
        let mut overlays = Overlays::default();
        let page = at(100.0, 100.0, 400.0, 300.0);
        assert!(!overlays.covers(page));
        overlays.0.push(at(0.0, 0.0, 90.0, 40.0));
        assert!(!overlays.covers(page), "a tooltip beside the page");
        overlays.0.push(at(450.0, 380.0, 120.0, 30.0));
        assert!(overlays.covers(page), "a tooltip over its corner");
    }

    #[test]
    fn an_overlay_that_only_touches_the_edge_of_the_page_does_not_cover_it() {
        let mut overlays = Overlays::default();
        overlays.0.push(at(100.0, 60.0, 80.0, 40.0));
        assert!(!overlays.covers(at(100.0, 100.0, 400.0, 300.0)));
    }
}
