//! Application start-up: fonts, shortcuts, menus and the one window.

use crate::{
    fonts,
    workbench::{NewTerminal, Paste, Quit, Workbench},
};
use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowOptions, point, px, size,
};
use gpui_platform::application;
use soyeht_client::{Engine, profile};

const WINDOW_SIZE: (f32, f32) = (1280.0, 800.0);
/// Where the window buttons sit; the rail leaves room for them.
const TRAFFIC_LIGHTS: (f32, f32) = (16.0, 16.0);

/// The platform key: Command on macOS, Control+Shift elsewhere, so that shortcuts never
/// collide with the keys a terminal program needs.
#[cfg(target_os = "macos")]
const MODIFIER: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const MODIFIER: &str = "ctrl-shift";

/// Opens the window and runs until the application quits.
pub fn run() {
    let socket = match profile::engine_socket() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("soyeht: {error}");
            std::process::exit(2);
        }
    };
    application().run(move |cx: &mut App| {
        fonts::register(cx);
        let (engine, inbox) = Engine::start(socket);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new(&format!("{MODIFIER}-q"), Quit, None),
            KeyBinding::new(&format!("{MODIFIER}-t"), NewTerminal, Some("Workbench")),
            KeyBinding::new(&format!("{MODIFIER}-v"), Paste, Some("Workbench")),
        ]);
        cx.set_menus([
            Menu::new("Soyeht").items([MenuItem::action("Quit Soyeht", Quit)]),
            Menu::new("Terminal").items([
                MenuItem::action("New Terminal", NewTerminal),
                MenuItem::action("Paste", Paste),
            ]),
        ]);
        let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Soyeht".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(TRAFFIC_LIGHTS.0), px(TRAFFIC_LIGHTS.1))),
            }),
            ..WindowOptions::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let workbench = cx.new(|cx| Workbench::new(engine, inbox, cx));
            let focus = workbench.read(cx).focus.clone();
            window.focus(&focus, cx);
            workbench
        });
        if let Err(error) = opened {
            eprintln!("soyeht: the window could not be opened: {error}");
            cx.quit();
        }
        cx.activate(true);
    });
}
