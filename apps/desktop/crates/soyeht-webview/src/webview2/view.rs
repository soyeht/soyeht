//! One page: a WebView2 controller in a host window, its state, and what it hands out.

use super::{
    handlers,
    host::{self},
    thread::{self, Ui},
    wide::{pcwstr, wide},
};
use crate::{
    Error, Navigation, PageState, Placement, Result, ScrollInput, ViewOptions, shared::Shared,
};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, CreateCoreWebView2EnvironmentWithOptions,
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Environment,
        ICoreWebView2Environment10, ICoreWebView2EnvironmentOptions,
    },
    take_pwstr,
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, RECT},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
            },
        },
    },
    core::{BOOL, Interface, PCWSTR, PWSTR},
};

/// A focus this soon after the program asked for it is the program's.
const FOCUS_ASKED_FOR: Duration = Duration::from_millis(500);
/// How long a page may take to come up.
const CREATE_TIMEOUT: Duration = Duration::from_secs(30);

/// What a navigation loads, with the address as the text the web view takes.
pub(super) type Target = crate::shared::Target<Vec<u16>>;

/// The target of `navigation`.
pub(super) fn target(navigation: Navigation<'_>) -> Result<Target> {
    Target::new(navigation, |url| (!url.contains('\0')).then(|| wide(url)))
}

/// The objects of one view, and what the handlers of its events need.
pub(crate) struct ViewCtx {
    pub(super) shared: Arc<Shared>,
    pub(super) host: HWND,
    pub(super) controller: ICoreWebView2Controller,
    pub(super) webview: ICoreWebView2,
    /// Where the page was last placed, for when the window it sits over moves.
    pub(super) placement: Cell<Option<Placement>>,
    pub(super) loading: Cell<bool>,
    /// Whether the page is on screen for the person, as the last placement said.
    shown: Cell<bool>,
    /// The size the page lays out for, in device pixels.
    size: Cell<(i32, i32)>,
    /// When the program last asked the page for the keyboard, to tell that focus from the person's.
    focus_asked: Cell<Option<Instant>>,
}

impl ViewCtx {
    pub(super) fn navigate(&self, target: &Target) {
        // SAFETY: the web view is alive for as long as this context is in the thread's table.
        let result = unsafe {
            match target {
                Target::Url(url) => self.webview.Navigate(pcwstr(url)),
                Target::Back => self.webview.GoBack(),
                Target::Forward => self.webview.GoForward(),
                Target::Reload => self.webview.Reload(),
                Target::ReloadIgnoringCache => return super::script::reload_ignoring_cache(self),
            }
        };
        if result.is_err() {
            // A step back or forward with nowhere to go is refused by the web view; the buttons
            // that ask are disabled then, and the state says why.
        }
    }

    /// The size the page lays out for, before it is placed.
    pub(super) fn resize(&self, width: u32, height: u32) {
        if self.placement.get().is_some() {
            return;
        }
        let (width, height) = (
            i32::try_from(width.clamp(1, 16_384)).unwrap_or(1),
            i32::try_from(height.clamp(1, 16_384)).unwrap_or(1),
        );
        self.fit(0, 0, width, height);
    }

    /// Gives the page the rectangle `width` by `height` at (`x`, `y`) of its host.
    fn fit(&self, x: i32, y: i32, width: i32, height: i32) {
        self.size.set((width, height));
        let bounds = RECT {
            left: x,
            top: y,
            right: x.saturating_add(width),
            bottom: y.saturating_add(height),
        };
        // SAFETY: the controller is alive; the rectangle is the call's own copy.
        if unsafe { self.controller.SetBounds(bounds) }.is_err() {
            // A controller that is closing takes no new size.
        }
    }

    pub(super) fn place(&self, ui: &Ui, placement: &Placement) {
        let first = self.placement.replace(Some(*placement)).is_none();
        let found = host::place(self.host, placement);
        self.shown.set(found.shown);
        self.fit(
            found.page.x,
            found.page.y,
            found.page.width,
            found.page.height,
        );
        // SAFETY: the controller is alive.
        unsafe {
            if self.controller.SetIsVisible(found.shown).is_err() {
                // A controller that is closing is not shown.
            }
            if self.controller.NotifyParentWindowPositionChanged().is_err() {
                // The same.
            }
        }
        if first || placement.visible {
            host::follow_owner(ui, placement.window);
        }
        host::release_owners(ui);
    }

    /// Makes a page that is not on screen draw, off every screen, because the engine draws a
    /// picture only of a page that shows. Undone by [`ViewCtx::conceal`].
    pub(super) fn reveal(&self) -> bool {
        if self.shown.get() {
            return false;
        }
        let (width, height) = self.size.get();
        host::reveal_off_screen(self.host, width, height);
        self.fit(0, 0, width, height);
        // SAFETY: the controller is alive.
        if unsafe { self.controller.SetIsVisible(true) }.is_err() {
            // A controller that is closing draws nothing.
        }
        true
    }

    /// Puts a page that [`ViewCtx::reveal`] showed back as the last placement said.
    pub(super) fn conceal(&self) {
        let Some(placement) = self.placement.get() else {
            host::hide(self.host);
            // SAFETY: the controller is alive.
            if unsafe { self.controller.SetIsVisible(false) }.is_err() {
                // A controller that is closing draws nothing.
            }
            return;
        };
        let found = host::place(self.host, &placement);
        self.fit(
            found.page.x,
            found.page.y,
            found.page.width,
            found.page.height,
        );
        // SAFETY: the controller is alive.
        if unsafe { self.controller.SetIsVisible(found.shown) }.is_err() {
            // A controller that is closing draws nothing.
        }
    }

    /// The window the page sits over moved: the page goes with it.
    pub(super) fn follow(&self) {
        if let Some(placement) = self.placement.get() {
            host::place(self.host, &placement);
        }
    }

    pub(super) fn focus(&self, focused: bool) {
        if !focused {
            return;
        }
        self.focus_asked.set(Some(Instant::now()));
        // The keyboard moves to the page only while the program has it; a page asked to take it
        // from another program waits.
        // SAFETY: plain queries and requests about windows, which the system checks.
        unsafe {
            let foreground = GetForegroundWindow();
            let mut process = 0;
            GetWindowThreadProcessId(foreground, Some(&raw mut process));
            if process == windows::Win32::System::Threading::GetCurrentProcessId() {
                if SetForegroundWindow(self.host).as_bool() {
                    // The page's window is in front; its own focus follows.
                }
                if self
                    .controller
                    .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
                    .is_err()
                {
                    // A page that is closing takes no focus.
                }
            }
        }
    }

    /// One scroll of the wheel at a point of the page, as the page's own scrolling: the page sees
    /// scroll events, as it does when the Mac's view scrolls by a page (ADR 0109). A wheel message
    /// posted to the page's window is not used: the engine routes it by where the person's own
    /// pointer is.
    pub(super) fn wheel(&self, scroll: &ScrollInput) {
        super::script::scroll(self, scroll);
    }

    /// Whether the page's focus is the one the program just asked for.
    pub(super) fn focus_was_asked_for(&self) -> bool {
        self.focus_asked
            .get()
            .is_some_and(|at| at.elapsed() < FOCUS_ASKED_FOR)
    }

    /// Device pixels in one logical pixel of the page's screen, as a whole number.
    pub(super) fn device_scale(&self) -> u32 {
        // SAFETY: a plain query about a window.
        let dpi = unsafe { GetDpiForWindow(self.host) };
        (dpi.max(96) + 48) / 96
    }

    /// Refreshes the page's state and tells the window when it changed.
    pub(super) fn refresh_state(&self) {
        let state = PageState {
            url: self.read_text(|webview, text| {
                // SAFETY: the web view is alive; the string is freed by `read_text`.
                unsafe { webview.Source(text) }
            }),
            title: self.read_text(|webview, text| {
                // SAFETY: as above.
                unsafe { webview.DocumentTitle(text) }
            }),
            loading: self.loading.get(),
            progress: if self.loading.get() { 0.5 } else { 1.0 },
            can_go_back: self.flag(|webview, flag| {
                // SAFETY: as above.
                unsafe { webview.CanGoBack(flag) }
            }),
            can_go_forward: self.flag(|webview, flag| {
                // SAFETY: as above.
                unsafe { webview.CanGoForward(flag) }
            }),
        };
        self.shared.set_state(state);
    }

    fn read_text(
        &self,
        read: impl FnOnce(&ICoreWebView2, *mut PWSTR) -> windows::core::Result<()>,
    ) -> String {
        let mut text = PWSTR::null();
        match read(&self.webview, &raw mut text) {
            Ok(()) if !text.is_null() => take_pwstr(text),
            _ => String::new(),
        }
    }

    fn flag(
        &self,
        read: impl FnOnce(&ICoreWebView2, *mut BOOL) -> windows::core::Result<()>,
    ) -> bool {
        let mut flag = BOOL(0);
        read(&self.webview, &raw mut flag).is_ok() && flag.as_bool()
    }
}

/// Makes a view and puts it in the thread's table.
pub(super) fn create(ui: &Ui, id: u64, options: &ViewOptions, shared: Arc<Shared>) -> Result<()> {
    let private = options.profile.is_none();
    let folder = match &options.profile {
        Some(profile) => profile.join("web"),
        None => private_folder_path(),
    };
    let environment = match ui.environment(&folder) {
        Some(environment) => environment,
        None => {
            let made = environment(&folder)?;
            ui.keep_environment(folder.clone(), made.clone());
            made
        }
    };
    let (width, height) = (
        i32::try_from(options.width.clamp(1, 16_384)).unwrap_or(1),
        i32::try_from(options.height.clamp(1, 16_384)).unwrap_or(1),
    );
    let host = host::create(width, height)?;
    let controller = match controller(&environment, host, private) {
        Ok(controller) => controller,
        Err(error) => {
            host::destroy(host);
            return Err(error);
        }
    };
    let made = ViewCtx::new(shared, host, controller);
    match made {
        Ok(view) => {
            let view = Rc::new(view);
            if let Err(error) = handlers::connect(&view) {
                view.dispose();
                return Err(error);
            }
            view.fit(0, 0, width, height);
            ui.add(id, view);
            Ok(())
        }
        Err(error) => {
            host::destroy(host);
            Err(error)
        }
    }
}

impl ViewCtx {
    /// Ends the page and its window.
    fn dispose(&self) {
        // SAFETY: the controller is alive until this context is dropped; closing it ends the page.
        if unsafe { self.controller.Close() }.is_err() {
            // A page that has closed already is closed.
        }
        host::destroy(self.host);
    }

    fn new(shared: Arc<Shared>, host: HWND, controller: ICoreWebView2Controller) -> Result<Self> {
        // SAFETY: the controller is alive; the web view it gives is a reference of its own.
        let webview = unsafe { controller.CoreWebView2() }
            .map_err(|_| Error::Unavailable("the web view could not be made"))?;
        Ok(Self {
            shared,
            host,
            controller,
            webview,
            placement: Cell::new(None),
            loading: Cell::new(false),
            shown: Cell::new(false),
            size: Cell::new((1, 1)),
            focus_asked: Cell::new(None),
        })
    }
}

/// The environment of a user data folder, made on this thread.
fn environment(folder: &Path) -> Result<ICoreWebView2Environment> {
    std::fs::create_dir_all(folder)
        .map_err(|_| Error::Unavailable("the web profile directory cannot be made"))?;
    let options = CoreWebView2EnvironmentOptions::default();
    // SAFETY: the setters write the options' own cells, which nothing else reads yet.
    unsafe {
        // The person's Windows account is not offered to the sites of a pane.
        options.set_allow_single_sign_on_using_os_primary_account(false);
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let (done, answer) = mpsc::channel();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            let value = result.map_err(|_| ()).and_then(|()| environment.ok_or(()));
            if done.send(value).is_err() {
                // The caller gave up waiting.
            }
            Ok(())
        },
    ));
    let folder = wide(&folder.to_string_lossy());
    // SAFETY: the strings and the options outlive the call, which copies what it keeps.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            pcwstr(&folder),
            &options,
            &handler,
        )
    }
    .map_err(|_| Error::Unavailable("the web view runtime could not be started"))?;
    thread::wait_for(&answer, CREATE_TIMEOUT)?
        .map_err(|()| Error::Unavailable("the web view runtime could not be started"))
}

/// The controller of a page in `host`, private when the pane keeps nothing.
fn controller(
    environment: &ICoreWebView2Environment,
    host: HWND,
    private: bool,
) -> Result<ICoreWebView2Controller> {
    let (done, answer) = mpsc::channel();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |result, controller| {
            let value = result.map_err(|_| ()).and_then(|()| controller.ok_or(()));
            if done.send(value).is_err() {
                // The caller gave up waiting.
            }
            Ok(())
        },
    ));
    // SAFETY: the host window is alive and this thread's; the handler is the call's own.
    let started = unsafe {
        if private {
            environment
                .cast::<ICoreWebView2Environment10>()
                .and_then(|environment| {
                    let options = environment.CreateCoreWebView2ControllerOptions()?;
                    options.SetIsInPrivateModeEnabled(true)?;
                    environment.CreateCoreWebView2ControllerWithOptions(host, &options, &handler)
                })
        } else {
            environment.CreateCoreWebView2Controller(host, &handler)
        }
    };
    started.map_err(|_| Error::Unavailable("the web view could not be made"))?;
    thread::wait_for(&answer, CREATE_TIMEOUT)?
        .map_err(|()| Error::Unavailable("the web view could not be made"))
}

pub(super) fn destroy(ui: &Ui, id: u64) {
    let Some(view) = ui.remove(id) else {
        return;
    };
    view.dispose();
    host::release_owners(ui);
}

/// What the name of a private pane's folder starts with; the process that made it ends it.
const PRIVATE_PREFIX: &str = "soyeht-next-private-";

/// The folder a private pane's page keeps what it needs while it runs. Nothing the person did in
/// the pane is kept in it, but the engine's own files are, so it goes when the program ends
/// ([`remove_stale_private_folders`]).
pub(super) fn private_folder_path() -> PathBuf {
    std::env::temp_dir().join(format!("{PRIVATE_PREFIX}{}", std::process::id()))
}

/// Removes the folders that private panes of programs that have ended left behind.
pub(super) fn remove_stale_private_folders() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(process) = name
            .strip_prefix(PRIVATE_PREFIX)
            .and_then(|pid| pid.parse::<u32>().ok())
        else {
            continue;
        };
        if process != std::process::id()
            && !process_is_alive(process)
            && std::fs::remove_dir_all(entry.path()).is_err()
        {
            // A file of it is still held: the next start tries again.
        }
    }
}

/// What `GetExitCodeProcess` says of a process that has not ended.
const STILL_ACTIVE: u32 = 259;

/// Whether the process is still running: a process that has ended is not, though another program
/// may still hold a handle that keeps its number from going away.
fn process_is_alive(process: u32) -> bool {
    // SAFETY: a request for a limited handle of a process, which the system may refuse.
    let opened = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) };
    opened.is_ok_and(|handle| {
        let mut code = 0_u32;
        // SAFETY: the handle was just made, and `code` is this call's own.
        let asked = unsafe { GetExitCodeProcess(handle, &raw mut code) };
        // SAFETY: the handle is closed once.
        if unsafe { CloseHandle(handle) }.is_err() {
            // A handle that cannot be closed is not one worth another word.
        }
        // A process whose state cannot be read is taken as running: its folder stays.
        asked.is_err() || code == STILL_ACTIVE
    })
}
