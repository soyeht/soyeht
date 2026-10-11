//! Small pieces every view uses.

use crate::{
    assets::Icon,
    theme::{Palette, desktop},
};
use gpui::{
    AnyElement, Div, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Svg, div, linear_color_stop,
    linear_gradient, px, rgb, svg,
};

/// A line icon of `size` points in a palette color.
pub fn icon(icon: Icon, size: f32, color: u32) -> Svg {
    svg()
        .path(icon.path())
        .size(px(size))
        .flex_none()
        .text_color(rgb(color))
}

/// A provider logo of `size` points in a palette color.
pub fn logo(path: &'static str, size: f32, color: u32) -> Svg {
    svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(rgb(color))
}

/// A stable element id from a kind and the thing it belongs to.
pub fn id(kind: &str, owner: &str) -> SharedString {
    SharedString::from(format!("{kind}-{owner}"))
}

/// A row whose children are centered on the cross axis.
pub fn row() -> Div {
    div().flex().flex_row().items_center()
}

/// A column.
pub fn column() -> Div {
    div().flex().flex_col()
}

/// An element that fills the room it is given and shows nothing, to push its siblings apart.
pub fn spacer() -> Div {
    div().flex_1().min_w_0()
}

/// A thin horizontal rule in `color`.
pub fn rule(color: u32, thickness: f32) -> AnyElement {
    div()
        .w_full()
        .h(px(thickness))
        .flex_none()
        .bg(rgb(color))
        .into_any_element()
}

/// A press on an element that is its own: it does not reach what is behind the element (the
/// pane's focus handler, the window drag), so the element's click is all the press does.
pub trait OwnPress: InteractiveElement + Sized {
    fn own_press(self) -> Self {
        self.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}

impl<T: InteractiveElement> OwnPress for T {}

/// How far a sideways scroller must be from an end before that end shows its fade.
const FADE_THRESHOLD: f32 = 0.5;

/// `content` in a scroller that moves only sideways, with a fade of the canvas color over each
/// end that hides more, so that a clipped control reads as scrollable. The wheel moves it only
/// sideways: a vertical turn belongs to whatever vertical scroller is under the pointer, a
/// transcript or a terminal, and must not also pan this one. `handle` is the scroller's own
/// scroll state; a scroller that has not been laid out yet shows no fade. The result fills the
/// room it is given, and the caller may size it further.
pub fn sideways_scroller(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    content: impl IntoElement,
    palette: &Palette,
) -> Div {
    let mut scroller = div()
        .id(id)
        .flex()
        .size_full()
        .overflow_x_scroll()
        .track_scroll(handle)
        .child(content);
    scroller.style().restrict_scroll_to_axis = Some(true);
    div()
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .child(scroller)
        .children(horizontal_fades(handle, palette))
}

/// A fade of the canvas color over each end of a sideways scroller that hides more, so that a
/// clipped control reads as scrollable. Place the result inside the `relative` parent of the
/// scroller, after it. The handle is the scroller's own `track_scroll` handle; a scroller that
/// has not been laid out yet shows no fade.
fn horizontal_fades(handle: &ScrollHandle, palette: &Palette) -> Vec<AnyElement> {
    let offset = f32::from(handle.offset().x);
    let hidden_right = f32::from(handle.max_offset().x) + offset;
    let solid: Hsla = rgb(palette.canvas).into();
    let clear = Hsla { a: 0.0, ..solid };
    let fade = |from: Hsla, to: Hsla| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(desktop::FADE_WIDTH))
            .bg(linear_gradient(
                90.0,
                linear_color_stop(from, 0.0),
                linear_color_stop(to, 1.0),
            ))
    };
    let mut fades = Vec::new();
    if offset < -FADE_THRESHOLD {
        fades.push(fade(solid, clear).left_0().into_any_element());
    }
    if hidden_right > FADE_THRESHOLD {
        fades.push(fade(clear, solid).right_0().into_any_element());
    }
    fades
}
