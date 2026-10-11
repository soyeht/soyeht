//! Application start-up: fonts, shortcuts, menus and the one window.

use crate::{
    assets::Assets,
    fonts,
    theme::{ThemeName, desktop},
    window_size::{self, opening_size},
    workbench::{NewTerminal, Paste, Quit, Workbench},
};
use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowDecorations, WindowOptions, point, px, size,
};
use gpui_platform::application;
use soyeht_client::{Engine, Event, profile};
use std::time::Instant;

/// Where the macOS window buttons sit; the top bar leaves room for them.
const TRAFFIC_LIGHTS: (f32, f32) = (desktop::TRAFFIC_LIGHT_INSET, desktop::TRAFFIC_LIGHT_INSET);

/// The application's identity to the window system: Wayland's `app_id`, and the class X11 window
/// managers group windows by. Without it a compositor shows the window as nameless and cannot
/// match a rule or an icon to it.
const APP_ID: &str = "soyeht";

/// The platform key: Command on macOS, Control+Shift elsewhere, so that shortcuts never
/// collide with the keys a terminal program needs.
#[cfg(target_os = "macos")]
const MODIFIER: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const MODIFIER: &str = "ctrl-shift";

/// What the window starts from: the engine it talks to, the events it draws, and how it opens.
pub(crate) struct Setup {
    pub engine: Engine,
    pub inbox: async_channel::Receiver<Event>,
    pub theme: ThemeName,
    /// A menu that is open from the first frame.
    pub menu: Option<crate::open_menu::Menu>,
    /// The window's size in points, when it is not the one the display suggests.
    pub size: Option<(f32, f32)>,
}

/// Opens the window and runs until the application quits.
pub fn run() {
    let socket = match profile::engine_socket() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("soyeht: {error}");
            std::process::exit(2);
        }
    };
    launch(move || {
        let (engine, inbox) = Engine::start(socket);
        Setup {
            engine,
            inbox,
            theme: ThemeName::DEFAULT,
            menu: None,
            size: None,
        }
    });
}

/// Opens the window on what `setup` says, once the application is running.
pub(crate) fn launch(setup: impl FnOnce() -> Setup + 'static) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        fonts::register(cx);
        let setup = setup();
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
        let display = cx
            .primary_display()
            .map(|display| display.bounds().size)
            .map(|extent| (f32::from(extent.width), f32::from(extent.height)));
        let (width, height) = setup.size.unwrap_or_else(|| opening_size(display));
        let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(window_size::MINIMUM.0), px(window_size::MINIMUM.1))),
            // The application draws its own title bar and window buttons: the top bar.
            window_decorations: Some(WindowDecorations::Client),
            app_id: Some(APP_ID.into()),
            titlebar: Some(TitlebarOptions {
                title: Some("Soyeht".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(TRAFFIC_LIGHTS.0), px(TRAFFIC_LIGHTS.1))),
            }),
            ..WindowOptions::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let workbench = cx.new(|cx| {
                let mut workbench = Workbench::new(setup.engine, setup.inbox, cx);
                workbench.theme = setup.theme;
                if let Some(menu) = setup.menu {
                    workbench.menus.toggle(menu, Instant::now());
                }
                workbench
            });
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
