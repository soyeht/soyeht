//! Application start-up: fonts, shortcuts, menus and the one window.

use crate::{
    assets::Assets,
    folder::OpenFolder,
    fonts, graphics, logging,
    theme::{ThemeName, desktop},
    window_size::{self, opening_size},
    workbench::{CopySelection, NewTerminal, Paste, Quit, Workbench},
};
use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowDecorations, WindowOptions, point, px, size,
};
use gpui_platform::application;
use soyeht_client::{Engine, Event, autostart::Autostart, profile};

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

/// The tasks surface a replay opens on.
pub(crate) struct TasksStart {
    pub view: crate::tasks_ui::TaskView,
    /// The task whose detail is open.
    pub task: Option<String>,
    /// The new-task form is open.
    pub new_task: bool,
}

/// What the window starts from: the engine it talks to, the events it draws, and how it opens.
pub(crate) struct Setup {
    pub engine: Engine,
    pub inbox: async_channel::Receiver<Event>,
    pub theme: ThemeName,
    /// A menu that is open from the first frame.
    pub menu: Option<crate::open_menu::Menu>,
    /// The Preferences panel that shows when Preferences are that menu.
    pub panel: Option<crate::prefs::Section>,
    /// The window's size in points, when it is not the one the display suggests.
    pub size: Option<(f32, f32)>,
    /// Files already attached in the composers, for a replay that draws them.
    pub staged: Vec<(String, soyeht_client::attachments::Staged)>,
    /// The tasks surface the window opens on, instead of the panes.
    pub tasks: Option<TasksStart>,
    /// The place of the rail the window opens on, when it is not the first.
    pub destination: Option<crate::destination::Destination>,
}

/// Where the application prints what it tells the person (the program's `main` owns the
/// terminal): advice such as which driver to install. The same text is a log line too
/// ([`logging::announce`]), since a program with no terminal prints it nowhere.
pub type Notice = fn(&str);

/// Opens the window and runs until the application quits.
pub fn run(notice: Notice) {
    logging::start();
    let socket = match profile::engine_socket() {
        Ok(path) => path,
        Err(error) => {
            logging::announce_fatal(notice, "profile_unavailable", &format!("soyeht: {error}"));
            std::process::exit(2);
        }
    };
    launch(notice, move || {
        let (engine, inbox) = Engine::start_with(socket, Autostart::resolve());
        Setup {
            engine,
            inbox,
            theme: ThemeName::DEFAULT,
            menu: None,
            panel: None,
            size: None,
            staged: Vec::new(),
            tasks: None,
            destination: None,
        }
    });
}

/// Opens the window on what `setup` says, once the application is running.
pub(crate) fn launch(notice: Notice, setup: impl FnOnce() -> Setup + 'static) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        fonts::register(cx);
        let setup = setup();
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new(&format!("{MODIFIER}-q"), Quit, None),
            KeyBinding::new(&format!("{MODIFIER}-o"), OpenFolder, Some("Workbench")),
            KeyBinding::new(&format!("{MODIFIER}-t"), NewTerminal, Some("Workbench")),
            KeyBinding::new(&format!("{MODIFIER}-v"), Paste, Some("Workbench")),
            KeyBinding::new(&format!("{MODIFIER}-c"), CopySelection, Some("Workbench")),
        ]);
        cx.set_menus([
            Menu::new("Soyeht").items([MenuItem::action("Quit Soyeht", Quit)]),
            Menu::new("File").items([MenuItem::action("Open Folder\u{2026}", OpenFolder)]),
            Menu::new("Terminal").items([
                MenuItem::action("New Terminal", NewTerminal),
                MenuItem::action("Copy", CopySelection),
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
                workbench.prefs.section = setup.panel;
                for (pane_id, staged) in setup.staged {
                    workbench.panes.of(&pane_id).staged = Some(staged);
                }
                if let Some(place) = setup.destination {
                    workbench.destination = place;
                }
                if let Some(start) = setup.tasks {
                    workbench.destination = crate::destination::Destination::Tasks;
                    workbench.tasks_ui.view = start.view;
                    workbench.tasks_ui.selected = start.task;
                    if start.new_task {
                        workbench.open_new_task(cx);
                    }
                }
                if let Some(menu) = setup.menu {
                    workbench.toggle_menu(menu, cx);
                }
                workbench
            });
            let focus = workbench.read(cx).focus.clone();
            window.focus(&focus, cx);
            if let Some(note) = graphics::software_note(window.gpu_specs().as_ref()) {
                logging::announce(notice, &note);
                // The window shows it too, in a line a connection does not clear.
                workbench.update(cx, |workbench, _| workbench.state.notice = Some(note));
            }
            workbench
        });
        if let Err(error) = opened {
            logging::announce_fatal(
                notice,
                "window_open_failed",
                &graphics::open_failure(&error, cfg!(target_os = "linux")),
            );
            cx.quit();
        }
        cx.activate(true);
    });
}
