//! Window controls for the platforms where the title bar is drawn by the application: minimize,
//! maximize and close as round raised buttons at the end of the top bar, and the empty part of
//! the bar as the area that drags the window.
//!
//! On Windows the system hit-tests these areas, so snapping and the caption menu work as in any
//! window. On Linux the buttons act on click and the bar starts the move itself.
//!
//! A button is drawn only when the window system can act on it: a compositor that does not offer
//! minimize or maximize, and a tiling desktop (Hyprland, sway) that lays every window out itself
//! and ignores both, get only the close button. Those compositors still announce the capability,
//! so the desktop session's name decides. A window snapped to half of the screen on GNOME or KDE
//! has a tiled edge but can still be minimized and maximized: the edge only takes away the
//! resize handles.

use super::ui::{icon, id, row, top_bar_button};
use crate::workbench::Workbench;
use crate::{
    assets::Icon,
    theme::{Palette, desktop, elevation, layout, radius, space},
};
use gpui::{
    AnyElement, Context, Decorations, Div, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Styled, Window, WindowControlArea, WindowControls, px,
};
// Windows acts on the hit-test; elsewhere the click does the work.
#[cfg(not(target_os = "windows"))]
use gpui::{ClickEvent, StatefulInteractiveElement};
use std::ffi::OsStr;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Minimize,
    Maximize,
    Close,
}

/// The buttons to draw: close always, minimize and maximize when the window system offers them
/// and does not ignore them.
fn buttons(offered: WindowControls, tiling_desktop: bool) -> Vec<Control> {
    let mut all = Vec::new();
    if offered.minimize && !tiling_desktop {
        all.push(Control::Minimize);
    }
    if offered.maximize && !tiling_desktop {
        all.push(Control::Maximize);
    }
    all.push(Control::Close);
    all
}

/// The desktops (`XDG_CURRENT_DESKTOP`) whose compositor lays windows out itself.
const TILING_DESKTOPS: [&str; 14] = [
    "hyprland",
    "sway",
    "niri",
    "river",
    "i3",
    "bspwm",
    "dwl",
    "qtile",
    "awesome",
    "herbstluftwm",
    "xmonad",
    "leftwm",
    "spectrwm",
    "miracle-wm",
];

/// Whether `current`, a colon-separated `XDG_CURRENT_DESKTOP`, names a tiling desktop.
fn tiling_desktop(current: Option<&OsStr>) -> bool {
    current.and_then(OsStr::to_str).is_some_and(|names| {
        names.split(':').any(|name| {
            TILING_DESKTOPS
                .iter()
                .any(|tiling| name.eq_ignore_ascii_case(tiling))
        })
    })
}

/// How the window system places the window. The edges are only read on Linux, where the
/// resize handles are drawn.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
struct Placement {
    /// The session is a tiling desktop.
    tiling_desktop: bool,
    /// The compositor has tiled an edge of the window.
    tiled_edge: bool,
    maximized: bool,
}

impl Placement {
    /// The placement of a window in the session named by `desktop`, `XDG_CURRENT_DESKTOP`.
    fn new(desktop: Option<&OsStr>, tiled_edge: bool, maximized: bool) -> Self {
        Self {
            tiling_desktop: tiling_desktop(desktop),
            tiled_edge,
            maximized,
        }
    }

    fn of(window: &Window) -> Self {
        Self::new(
            std::env::var_os("XDG_CURRENT_DESKTOP").as_deref(),
            matches!(
                window.window_decorations(),
                Decorations::Client { tiling } if tiling.is_tiled()
            ),
            window.is_maximized(),
        )
    }

    /// Whether the person can drag an edge to size the window: not when the window system sizes
    /// it, as a tiling desktop or a tiled edge says, or when it fills the screen.
    #[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
    fn resizable(self) -> bool {
        !(self.tiling_desktop || self.tiled_edge || self.maximized)
    }
}

/// Whether the window's edges take a resize drag.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn resizable(window: &Window) -> bool {
    Placement::of(window).resizable()
}

/// The buttons the top bar draws for `window`.
pub fn visible(window: &Window) -> Vec<Control> {
    buttons(
        window.window_controls(),
        Placement::of(window).tiling_desktop,
    )
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
    shown: &[Control],
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    row()
        .flex_none()
        .gap(px(space::X10))
        .children(shown.iter().map(|&control| button(control, palette, cx)))
        .into_any_element()
}

#[cfg_attr(target_os = "windows", allow(unused_variables))]
fn button(control: Control, palette: &'static Palette, cx: &mut Context<Workbench>) -> AnyElement {
    let danger = matches!(control, Control::Close);
    let button = top_bar_button(
        palette,
        radius::PILL,
        elevation::PILL,
        layout::TOP_BAR_CONTROL_HEIGHT,
    )
    .id(id("window", control.name()))
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

#[cfg(test)]
mod tests {
    use super::*;

    const OFFERED: WindowControls = WindowControls {
        fullscreen: true,
        maximize: true,
        minimize: true,
        window_menu: true,
    };

    #[test]
    fn a_floating_window_gets_all_three_buttons() {
        assert_eq!(
            buttons(OFFERED, false),
            [Control::Minimize, Control::Maximize, Control::Close]
        );
    }

    #[test]
    fn a_managed_window_keeps_only_close() {
        assert_eq!(buttons(OFFERED, true), [Control::Close]);
    }

    #[test]
    fn a_button_the_compositor_does_not_offer_is_not_drawn() {
        let no_minimize = WindowControls {
            minimize: false,
            ..OFFERED
        };
        assert_eq!(
            buttons(no_minimize, false),
            [Control::Maximize, Control::Close]
        );
    }

    fn placement(desktop: &str, tiled_edge: bool, maximized: bool) -> Placement {
        Placement::new(Some(OsStr::new(desktop)), tiled_edge, maximized)
    }

    #[test]
    fn a_snapped_window_on_gnome_or_kde_keeps_minimize_and_maximize() {
        for desktop in ["ubuntu:GNOME", "KDE"] {
            let snapped = placement(desktop, true, false);
            assert_eq!(
                buttons(OFFERED, snapped.tiling_desktop),
                [Control::Minimize, Control::Maximize, Control::Close]
            );
            assert!(
                !snapped.resizable(),
                "a tiled edge takes the resize handles"
            );
        }
    }

    #[test]
    fn a_tiling_desktop_hides_minimize_and_maximize_and_the_resize_handles() {
        let hyprland = placement("Hyprland", false, false);
        assert_eq!(buttons(OFFERED, hyprland.tiling_desktop), [Control::Close]);
        assert!(!hyprland.resizable());
        // Even when it reports the window as maximized.
        assert_eq!(
            buttons(OFFERED, placement("sway", true, true).tiling_desktop),
            [Control::Close]
        );
    }

    #[test]
    fn a_floating_window_is_resizable_until_it_is_maximized() {
        assert!(placement("ubuntu:GNOME", false, false).resizable());
        assert!(!placement("ubuntu:GNOME", false, true).resizable());
        assert!(Placement::new(None, false, false).resizable());
    }

    #[test]
    fn a_tiling_desktop_is_told_by_its_session_name() {
        assert!(tiling_desktop(Some(OsStr::new("Hyprland"))));
        assert!(tiling_desktop(Some(OsStr::new("sway"))));
        assert!(tiling_desktop(Some(OsStr::new("X-Cinnamon:i3"))));
        assert!(!tiling_desktop(Some(OsStr::new("ubuntu:GNOME"))));
        assert!(!tiling_desktop(Some(OsStr::new("KDE"))));
        assert!(!tiling_desktop(None));
    }
}
