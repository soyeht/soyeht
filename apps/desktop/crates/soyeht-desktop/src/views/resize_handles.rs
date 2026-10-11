//! Resize edges for windows whose frame the application draws (Linux with client-side
//! decorations): thin strips along the edges and larger squares at the corners hand the drag to
//! the window system.

use gpui::{AnyElement, Window};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn handles(window: &Window) -> Vec<AnyElement> {
    use gpui::{
        CursorStyle, InteractiveElement, IntoElement, MouseButton, ResizeEdge, Styled, div, px,
    };

    use crate::theme::desktop::{RESIZE_CORNER as CORNER, RESIZE_EDGE as EDGE};
    // A maximized, full-screen or tiled window is sized by the window system, not by its edges.
    if window.is_fullscreen() || !super::window_controls::resizable(window) {
        return Vec::new();
    }
    let strip = |edge: ResizeEdge, cursor: CursorStyle, place: fn(gpui::Div) -> gpui::Div| {
        place(div().absolute())
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.start_window_resize(edge);
                cx.stop_propagation();
            })
            .into_any_element()
    };
    vec![
        strip(ResizeEdge::Top, CursorStyle::ResizeUpDown, |d| {
            d.top_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE))
        }),
        strip(ResizeEdge::Bottom, CursorStyle::ResizeUpDown, |d| {
            d.bottom_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE))
        }),
        strip(ResizeEdge::Left, CursorStyle::ResizeLeftRight, |d| {
            d.left_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE))
        }),
        strip(ResizeEdge::Right, CursorStyle::ResizeLeftRight, |d| {
            d.right_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE))
        }),
        strip(
            ResizeEdge::TopLeft,
            CursorStyle::ResizeUpLeftDownRight,
            |d| d.top_0().left_0().size(px(CORNER)),
        ),
        strip(
            ResizeEdge::TopRight,
            CursorStyle::ResizeUpRightDownLeft,
            |d| d.top_0().right_0().size(px(CORNER)),
        ),
        strip(
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
            |d| d.bottom_0().left_0().size(px(CORNER)),
        ),
        strip(
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
            |d| d.bottom_0().right_0().size(px(CORNER)),
        ),
    ]
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub fn handles(_window: &Window) -> Vec<AnyElement> {
    Vec::new()
}
