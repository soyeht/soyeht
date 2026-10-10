//! Small pieces every view uses.

use crate::assets::Icon;
use gpui::{
    AnyElement, Div, InteractiveElement, IntoElement, MouseButton, SharedString, Styled, Svg, div,
    px, rgb, svg,
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
