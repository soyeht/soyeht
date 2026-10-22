//! The pointer over the whole window while a drag lasts.

use gpui::{App, DispatchPhase, MouseMoveEvent, MouseUpEvent, Window};

/// Follows the pointer from here on, not only over the element that began the drag: `moved` gets
/// every move and `released` the button's release, wherever they happen in the window. Called
/// while painting, each frame the drag lasts.
pub(crate) fn follow(
    window: &mut Window,
    moved: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    released: impl Fn(&mut Window, &mut App) + 'static,
) {
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            moved(event, window, cx);
        }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            released(window, cx);
        }
    });
}
