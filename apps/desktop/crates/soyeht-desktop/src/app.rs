//! Application start-up: fonts, shortcuts, menus and the one window.

use crate::{
    assets::Assets,
    folder::OpenFolder,
    fonts, graphics, logging, notices, shortcuts,
    tasks_ui::{CreateAndDispatchTask, CreateTask},
    theme::{ThemeName, desktop},
    web_page::{Signal, WebRegistry},
    window_size::{self, opening_size},
    workbench::{CopySelection, NewTerminal, Paste, Quit, Workbench},
};
use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, PlatformDisplay, TitlebarOptions,
    WindowBounds, WindowDecorations, WindowOptions, point, px, size,
};
use gpui_platform::application;
use soyeht_client::{
    Engine, Event,
    autostart::Autostart,
    local_preferences::{self, LocalPreferences},
    profile,
};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

/// Where the macOS window buttons sit; the top bar leaves room for them.
const TRAFFIC_LIGHTS: (f32, f32) = (desktop::TRAFFIC_LIGHT_INSET, desktop::TRAFFIC_LIGHT_INSET);

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
    /// The application's identity to the window system: Wayland's `app_id`, and the class X11
    /// window managers group windows by. Without it a compositor shows the window as nameless
    /// and cannot match a rule or an icon to it. It is the profile's application id.
    pub app_id: &'static str,
    pub engine: Engine,
    pub inbox: async_channel::Receiver<Event>,
    pub theme: ThemeName,
    /// Where the person's own choices (the theme) are kept; `None` keeps none, for a replay.
    pub preferences: Option<std::path::PathBuf>,
    /// A menu that is open from the first frame.
    pub menu: Option<crate::open_menu::Menu>,
    /// The Preferences panel that shows when Preferences are that menu.
    pub panel: Option<crate::prefs::Section>,
    /// The window's size in points, when it is not the one the display suggests.
    pub size: Option<(f32, f32)>,
    /// Files already attached in the composers, for a replay that draws them.
    pub staged: Vec<(String, soyeht_client::attachments::Staged)>,
    /// Text already written in the composers, for a replay that draws a list over it.
    pub drafts: Vec<(String, String)>,
    /// The tasks surface the window opens on, instead of the panes.
    pub tasks: Option<TasksStart>,
    /// The place of the rail the window opens on, when it is not the first.
    pub destination: Option<crate::destination::Destination>,
    /// The web view of the window and the signals its pages raise; none where the system has
    /// no web view (a replay has none either).
    pub web: Option<(std::sync::Arc<WebRegistry>, async_channel::Receiver<Signal>)>,
}

/// Where the application prints what it tells the person (the program's `main` owns the
/// terminal): advice such as which driver to install. The same text is a log line too
/// ([`logging::announce`]), since a program with no terminal prints it nowhere.
pub type Notice = fn(&str);

/// Opens the window and runs until the application quits.
pub fn run(notice: Notice) {
    logging::start();
    let (socket, app_id) = match profile::engine_socket()
        .and_then(|socket| Ok((socket, profile::selected()?.linux_app_id)))
    {
        Ok(found) => found,
        Err(error) => {
            logging::announce_fatal(notice, "profile_unavailable", &format!("soyeht: {error}"));
            std::process::exit(2);
        }
    };
    launch(notice, move || {
        let preferences = local_preferences::directory(&socket);
        // Read before the window opens, so the first frame is already in the chosen theme.
        let theme = crate::prefs::saved_theme(&LocalPreferences::load(&preferences));
        // Where the web panes keep their cookies and storage: a login lasts across runs. A page
        // is the color of the pane's face until it has drawn.
        let web = soyeht_webview::supported().then(|| {
            let (signals, raised) = async_channel::unbounded();
            (
                WebRegistry::new(preferences.join("browser"), theme.palette().face, signals),
                raised,
            )
        });
        // The window's web panes are the engine's web host: its agents read them.
        let host = web.as_ref().map(|(registry, _)| {
            std::sync::Arc::new(soyeht_client::web_host::Host::new(registry.clone()))
        });
        let (engine, inbox) = Engine::start_hosted(socket.clone(), Autostart::resolve(), host);
        Setup {
            app_id,
            engine,
            inbox,
            theme,
            preferences: Some(preferences),
            menu: None,
            panel: None,
            size: None,
            staged: Vec::new(),
            drafts: Vec::new(),
            tasks: None,
            destination: None,
            web,
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
            KeyBinding::new(&shortcuts::QUIT.binding(), Quit, None),
            KeyBinding::new(
                &shortcuts::OPEN_FOLDER.binding(),
                OpenFolder,
                Some("Workbench"),
            ),
            KeyBinding::new(
                &shortcuts::NEW_TERMINAL.binding(),
                NewTerminal,
                Some("Workbench"),
            ),
            KeyBinding::new(&shortcuts::PASTE.binding(), Paste, Some("Workbench")),
            KeyBinding::new(&shortcuts::COPY.binding(), CopySelection, Some("Workbench")),
            KeyBinding::new(
                &shortcuts::CREATE_TASK.binding(),
                CreateTask,
                Some("Workbench"),
            ),
            KeyBinding::new(
                &shortcuts::CREATE_AND_DISPATCH_TASK.binding(),
                CreateAndDispatchTask,
                Some("Workbench"),
            ),
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
        let known = || cx.primary_display().is_some() || !cx.displays().is_empty();
        if setup.size.is_some() || known() {
            open_window(cx, setup, notice);
        } else {
            open_when_displays_are_known(cx, setup, notice);
        }
    });
}

/// How long the window waits for the window system to name a display, and how often it asks.
const DISPLAY_WAIT: Duration = Duration::from_secs(2);
const DISPLAY_POLL: Duration = Duration::from_millis(20);

/// Opens the window once the window system has told the application its displays. A Wayland
/// session announces them only after the application has started: a window opened before then
/// is sized for no display, and is larger than a small screen holds, which the compositor then
/// maximizes, and restoring brings back a window wider than the screen. A session that never
/// names one opens the window at the design size after [`DISPLAY_WAIT`].
fn open_when_displays_are_known(cx: &mut App, setup: Setup, notice: Notice) {
    cx.spawn(async move |cx| {
        let started = Instant::now();
        while started.elapsed() < DISPLAY_WAIT
            && !cx.update(|cx| cx.primary_display().is_some() || !cx.displays().is_empty())
        {
            cx.background_executor().timer(DISPLAY_POLL).await;
        }
        cx.update(|cx| open_window(cx, setup, notice));
    })
    .detach();
}

/// The window, sized for the display it opens on, showing what `setup` says.
fn open_window(cx: &mut App, setup: Setup, notice: Notice) {
    let extent = |display: &Rc<dyn PlatformDisplay>| {
        let size = display.bounds().size;
        (f32::from(size.width), f32::from(size.height))
    };
    let display = window_size::display_to_fit(
        cx.primary_display().map(|display| extent(&display)),
        &cx.displays().iter().map(extent).collect::<Vec<_>>(),
    );
    let (width, height) = setup.size.unwrap_or_else(|| opening_size(display));
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(window_size::MINIMUM.0), px(window_size::MINIMUM.1))),
        // The application draws its own title bar and window buttons: the top bar.
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(setup.app_id.into()),
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
            workbench.preferences = setup.preferences;
            workbench.start_update_checks(cx);
            workbench.check_crash_reports(cx);
            workbench.prefs.section = setup.panel;
            for (pane_id, staged) in setup.staged {
                workbench.panes.of(&pane_id).staged = Some(staged);
            }
            for (pane_id, text) in setup.drafts {
                let field = workbench.composer_field(&pane_id, cx);
                field.update(cx, |field, cx| field.insert(&text, cx));
            }
            if let Some(place) = setup.destination {
                workbench.destination = place;
            }
            if let Some((registry, signals)) = setup.web {
                workbench.attach_web(registry, signals, cx);
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
        workbench.update(cx, |workbench, cx| workbench.listen_for_alerts(window, cx));
        if let Some(note) = graphics::software_note(window.gpu_specs().as_ref()) {
            logging::announce(notice, &note);
            // The window shows it too, in a line a connection does not clear.
            workbench.update(cx, |workbench, _| {
                workbench.show_notice(notices::SOFTWARE_RENDERER, note)
            });
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
}
