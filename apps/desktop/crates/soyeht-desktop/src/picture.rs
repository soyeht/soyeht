//! What a view that paints a picture it draws itself (a page of the web, an avatar) does the same
//! way: it paints in the place the layout gave it, swaps the picture it shows for the next one and
//! lets the old one go from the window's atlas.

use gpui::{App, Bounds, Canvas, Context, Corners, Entity, Pixels, RenderImage, Window, canvas};
use std::sync::Arc;

/// An element that calls `paint` on the view, at the end of the layout, with the place it was
/// given.
pub(crate) fn painted<V: 'static>(
    view: Entity<V>,
    paint: fn(&mut V, Bounds<Pixels>, &mut Window, &mut Context<V>),
) -> Canvas<Bounds<Pixels>> {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window: &mut Window, cx: &mut App| {
            view.update(cx, |view, cx| paint(view, bounds, window, cx));
        },
    )
}

/// Shows `next` in place of `shown`, and lets the window drop the picture that was shown.
pub(crate) fn swap_picture<T>(
    window: &mut Window,
    shown: &mut Option<T>,
    next: T,
    image_of: fn(T) -> Arc<RenderImage>,
) {
    if let Some(old) = shown.replace(next)
        && window.drop_image(image_of(old)).is_err()
    {
        // The picture was already gone from the atlas.
    }
}

/// Paints `image` at `at`.
pub(crate) fn paint_picture(window: &mut Window, at: Bounds<Pixels>, image: &Arc<RenderImage>) {
    if window
        .paint_image(at, Corners::default(), Arc::clone(image), 0, false)
        .is_err()
    {
        // The picture could not be uploaded; the next one is tried at the next frame.
    }
}
