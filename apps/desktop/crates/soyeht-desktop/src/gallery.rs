//! The neumorphic gallery: every raised and inset surface of the window chrome on one canvas,
//! at fixed positions, for comparing the desktop client with the Mac client pixel by pixel
//! (`examples/neo_gallery.rs`). It draws no text, so a difference is always a surface.

use crate::{
    neo,
    theme::{
        Palette, ThemeName,
        desktop::{self, WELL_DEPTH},
        elevation::{self, Elevation},
        pane_interior, pane_layout, radius,
    },
};
use gpui::{
    App, AppContext, Bounds, Context, Div, IntoElement, ParentElement, Render, Styled, Window,
    WindowBounds, WindowOptions, div, px, rgb, size,
};
use gpui_platform::application;

/// The canvas, in points.
pub const SIZE: (f32, f32) = (780.0, 530.0);

/// Where a surface sits: left, top, width, height.
type Place = (f32, f32, f32, f32);

fn at(surface: Div, (left, top, width, height): Place) -> Div {
    surface
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
}

fn flat(color: u32, corner: f32, place: Place) -> Div {
    at(div().bg(rgb(color)).rounded(px(corner)), place)
}

/// The elevation ladder, from the flattest to the highest.
const LADDER: [Elevation; 8] = [
    elevation::CHIP,
    elevation::TOGGLE,
    elevation::TASK_CARD,
    elevation::ACTION,
    elevation::CARD,
    elevation::PILL,
    elevation::PANEL,
    elevation::BALLOON,
];

struct Gallery {
    palette: &'static Palette,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let inset = pane_layout::PANE_CARD_INSET;
        let pane = (40.0, 110.0, 400.0, 300.0);
        let inner =
            |top: f32, height: f32| (pane.0 + inset, pane.1 + top, pane.2 - 2.0 * inset, height);
        div()
            .size_full()
            .bg(rgb(palette.canvas))
            .relative()
            .child(at(
                neo::raised(palette, radius::CONTROL, elevation::CARD),
                (40.0, 40.0, 34.0, 34.0),
            ))
            .child(at(
                neo::well(palette, radius::PILL, WELL_DEPTH),
                (100.0, 40.0, 125.0, 36.0),
            ))
            .child(at(
                neo::raised(palette, radius::PILL, elevation::PILL),
                (245.0, 40.0, 100.0, 36.0),
            ))
            .child(at(
                neo::raised(palette, radius::PILL, elevation::PILL),
                (365.0, 40.0, 34.0, 34.0),
            ))
            .child(at(
                neo::raised(palette, radius::PILL, elevation::PILL),
                (420.0, 40.0, 90.0, 34.0),
            ))
            .child(flat(
                palette.well,
                radius::COMPACT,
                (540.0, 40.0, 36.0, 36.0),
            ))
            .child(at(neo::pane_shell(palette), pane))
            .child(flat(palette.well, 0.0, inner(inset + 26.0 + 8.0, 1.0)))
            .child(flat(
                palette.canvas,
                pane_interior::CARD_RADIUS,
                inner(inset + 26.0 + 8.0 + 1.0 + 8.0, 70.0),
            ))
            .child(flat(
                palette.canvas,
                pane_interior::COMPOSER_RADIUS,
                inner(pane.3 - inset - 56.0, 56.0),
            ))
            .child(at(
                neo::raised(palette, desktop::USAGE_BALLOON_RADIUS, elevation::BALLOON),
                (500.0, 110.0, 200.0, 130.0),
            ))
            .child(at(
                neo::raised(palette, radius::CARD, elevation::PANEL),
                (500.0, 280.0, 200.0, 110.0),
            ))
            .children(LADDER.iter().enumerate().map(|(index, level)| {
                at(
                    neo::raised(palette, radius::CONTROL, *level),
                    (40.0 + index as f32 * 88.0, 450.0, 60.0, 40.0),
                )
            }))
    }
}

/// Opens one window with the gallery in `theme` and runs until it closes.
pub fn run(theme: ThemeName) {
    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(SIZE.0), px(SIZE.1)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            |_, cx| {
                cx.new(|_| Gallery {
                    palette: theme.palette(),
                })
            },
        );
        if opened.is_err() {
            cx.quit();
        }
        cx.activate(true);
    });
}
