//! Window controls for the platforms where the title bar is drawn by the application: minimize,
//! maximize and close as round raised buttons at the end of the top bar, and the empty part of
//! the bar as the area that drags the window.
//!
//! On Windows the system hit-tests these areas, so snapping and the caption menu work as in any
//! window. On Linux the buttons act on click and the bar starts the move itself.

use super::ui::{icon, id, row};
use crate::workbench::Workbench;
use crate::{
    assets::Icon,
    neo,
    theme::{Palette, desktop, elevation, layout, radius, space},
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, Window, WindowControlArea, px,
};

/// The width of the three buttons and the gaps between them.
pub const WIDTH: f32 = 3.0 * layout::TOP_BAR_CONTROL_HEIGHT + 2.0 * space::X10;

/// Makes `area` the part of the bar that moves the window; a double click maximizes.
pub fn drag_area(area: Div) -> AnyElement {
    area.window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .into_any_element()
}

enum Control {
    Minimize,
    Maximize,
    Close,
}

impl Control {
    fn name(&self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }

    fn area(&self) -> WindowControlArea {
        match self {
            Self::Minimize => WindowControlArea::Min,
            Self::Maximize => WindowControlArea::Max,
            Self::Close => WindowControlArea::Close,
        }
    }

    fn icon(&self) -> Icon {
        match self {
            Self::Minimize => Icon::Minimize,
            Self::Maximize => Icon::Maximize,
            Self::Close => Icon::Close,
        }
    }
}

pub fn controls(
    palette: &'static Palette,
    _window: &mut Window,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    row()
        .flex_none()
        .gap(px(space::X10))
        .children(
            [Control::Minimize, Control::Maximize, Control::Close]
                .into_iter()
                .map(|control| button(control, palette, cx)),
        )
        .into_any_element()
}

#[cfg_attr(target_os = "windows", allow(unused_variables))]
fn button(control: Control, palette: &'static Palette, cx: &mut Context<Workbench>) -> AnyElement {
    let danger = matches!(control, Control::Close);
    let button = neo::raised(palette, radius::PILL, elevation::PILL)
        .id(id("window", control.name()))
        .size(px(layout::TOP_BAR_CONTROL_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .window_control_area(control.area())
        .child(icon(
            control.icon(),
            desktop::WINDOW_CONTROL_GLYPH,
            if danger {
                palette.danger()
            } else {
                palette.ink
            },
        ));
    // Windows acts on the hit-test; elsewhere the click does the work.
    #[cfg(not(target_os = "windows"))]
    let button = button.on_click(
        cx.listener(move |_, _: &ClickEvent, window, cx| match control {
            Control::Minimize => window.minimize_window(),
            Control::Maximize => window.zoom_window(),
            Control::Close => cx.quit(),
        }),
    );
    button.into_any_element()
}
