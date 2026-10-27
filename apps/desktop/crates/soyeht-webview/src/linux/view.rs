//! One page: a WebKit view in an off-screen GTK window, its signals, and what it hands out.

use super::{
    Gtk,
    calls::{Pending, Target, copy_string},
    ffi::{self, Api, GBool, Ptr},
    private_directory,
};
use crate::{Error, Frame, PageState, Result, ViewOptions, ViewSink, ViewUpdate, policy};
use std::{
    cell::Cell,
    ffi::{CStr, CString, c_char, c_int, c_uint},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr,
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

/// The script world of everything the reader runs. The page has no handle on it.
const WORLD: &CStr = c"soyeht-read";
/// Pictures are read at most this often, in milliseconds (about 30 a second).
const FRAME_INTERVAL_MS: c_uint = 33;
/// `WEBKIT_COOKIE_PERSISTENT_STORAGE_SQLITE`.
const COOKIES_SQLITE: c_int = 1;
/// `WEBKIT_LOAD_STARTED`.
const LOAD_STARTED: c_int = 0;
/// `WEBKIT_POLICY_DECISION_TYPE_RESPONSE`.
const DECISION_RESPONSE: c_int = 2;
/// `WEBKIT_SCRIPT_DIALOG_CONFIRM` and `_BEFORE_UNLOAD_CONFIRM`.
const DIALOG_CONFIRM: c_int = 1;
const DIALOG_BEFORE_UNLOAD: c_int = 3;
/// `WEBKIT_SNAPSHOT_REGION_VISIBLE`, and no snapshot options.
const SNAPSHOT_VISIBLE: c_int = 0;
const SNAPSHOT_PLAIN: c_int = 0;

/// What the other threads see of a view.
pub(super) struct Shared {
    pub frame: Mutex<Option<Frame>>,
    pub state: Mutex<PageState>,
    pub navigations: AtomicU64,
    sink: ViewSink,
}

impl Shared {
    pub fn new(sink: ViewSink) -> Self {
        Self {
            frame: Mutex::new(None),
            state: Mutex::new(PageState::default()),
            navigations: AtomicU64::new(0),
            sink,
        }
    }

    fn tell(&self, update: ViewUpdate) {
        (self.sink)(update);
    }
}

/// The GTK objects of one view, and what the handlers of its signals need.
pub(crate) struct ViewCtx {
    pub(super) api: &'static Api,
    shared: Arc<Shared>,
    webview: Ptr,
    window: Ptr,
    cookies: Ptr,
    /// The context to release with the view: only a private view owns its context.
    private_context: Option<Ptr>,
    timer: Cell<c_uint>,
    /// The buttons held, as GDK's modifier bits, for the motion events of a drag.
    pub(super) buttons: Cell<u32>,
}

impl ViewCtx {
    pub(super) fn webview(&self) -> Ptr {
        self.webview
    }

    pub(super) fn window(&self) -> Ptr {
        self.window
    }

    fn data(&self) -> Ptr {
        ptr::from_ref(self).cast_mut().cast()
    }

    pub(super) fn navigate(&self, target: &Target) {
        // SAFETY: the web view is alive for as long as this context is in the thread's table.
        unsafe {
            match target {
                Target::Url(url) => (self.api.webkit_web_view_load_uri)(self.webview, url.as_ptr()),
                Target::Back => (self.api.webkit_web_view_go_back)(self.webview),
                Target::Forward => (self.api.webkit_web_view_go_forward)(self.webview),
                Target::Reload => (self.api.webkit_web_view_reload)(self.webview),
                Target::ReloadIgnoringCache => {
                    (self.api.webkit_web_view_reload_bypass_cache)(self.webview);
                }
            }
        }
    }

    pub(super) fn resize(&self, width: u32, height: u32) {
        let (width, height) = (clamp_extent(width), clamp_extent(height));
        // SAFETY: both widgets are alive for as long as this context is in the thread's table.
        unsafe {
            (self.api.gtk_widget_set_size_request)(self.webview, width, height);
            // A window that is not given its new default size keeps the old one as its least.
            (self.api.gtk_window_set_default_size)(self.window, width, height);
            (self.api.gtk_window_resize)(self.window, width, height);
        }
    }

    pub(super) fn edit_with(&self, command: &CStr, argument: Option<&CStr>) {
        // SAFETY: the web view is alive; the strings outlive the call, which copies them.
        unsafe {
            match argument {
                Some(argument) => (self
                    .api
                    .webkit_web_view_execute_editing_command_with_argument)(
                    self.webview,
                    command.as_ptr(),
                    argument.as_ptr(),
                ),
                None => (self.api.webkit_web_view_execute_editing_command)(
                    self.webview,
                    command.as_ptr(),
                ),
            }
        }
    }

    /// Reads the page's own surface into the picture the pane shows.
    fn read_live_frame(&self) {
        // SAFETY: the window is alive; its surface belongs to it and is read before returning.
        let frame = unsafe {
            let surface = (self.api.gtk_offscreen_window_get_surface)(self.window);
            read_surface(self.api, surface)
        };
        if let Some(frame) = frame {
            *self
                .shared
                .frame
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(frame);
            self.shared.tell(ViewUpdate::Frame);
        }
    }

    fn schedule_frame(&self) {
        if self.timer.get() != 0 {
            return;
        }
        // SAFETY: `frame_tick` runs on this thread and the timer is removed before the context
        // goes away (`destroy`).
        let id = unsafe {
            (self.api.g_timeout_add_full)(
                ffi::PRIORITY_DEFAULT,
                FRAME_INTERVAL_MS,
                frame_tick,
                self.data(),
                None,
            )
        };
        self.timer.set(id);
    }

    /// Refreshes the page's state and tells the window when it changed.
    fn refresh_state(&self) {
        // SAFETY: the web view is alive; the strings it returns are copied before the next call.
        let state = unsafe {
            let api = self.api;
            PageState {
                url: copy_string((api.webkit_web_view_get_uri)(self.webview)),
                title: copy_string((api.webkit_web_view_get_title)(self.webview)),
                loading: (api.webkit_web_view_is_loading)(self.webview) != ffi::FALSE,
                progress: (api.webkit_web_view_get_estimated_load_progress)(self.webview),
                can_go_back: (api.webkit_web_view_can_go_back)(self.webview) != ffi::FALSE,
                can_go_forward: (api.webkit_web_view_can_go_forward)(self.webview) != ffi::FALSE,
            }
        };
        let mut current = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *current != state {
            *current = state;
            drop(current);
            self.shared.tell(ViewUpdate::State);
        }
    }

    pub(super) fn capture(&self, reply: std::sync::mpsc::Sender<Result<Frame>>) {
        let pending = Pending {
            api: self.api,
            reply,
        }
        .into_raw();
        // SAFETY: `snapshot_ready` takes the pending call back, once, when the engine answers.
        unsafe {
            (self.api.webkit_web_view_get_snapshot)(
                self.webview,
                SNAPSHOT_VISIBLE,
                SNAPSHOT_PLAIN,
                ptr::null_mut(),
                snapshot_ready,
                pending,
            );
        }
    }

    pub(super) fn run_script(
        &self,
        source: &CString,
        reply: std::sync::mpsc::Sender<Result<String>>,
    ) {
        let pending = Pending {
            api: self.api,
            reply,
        }
        .into_raw();
        // SAFETY: the script is copied by the call; `script_ready` takes the pending call back.
        unsafe {
            (self.api.webkit_web_view_evaluate_javascript)(
                self.webview,
                source.as_ptr(),
                -1,
                WORLD.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                script_ready,
                pending,
            );
        }
    }

    pub(super) fn cookies_present(
        &self,
        url: &CString,
        reply: std::sync::mpsc::Sender<Result<bool>>,
    ) {
        let pending = Pending {
            api: self.api,
            reply,
        }
        .into_raw();
        // SAFETY: the address is copied by the call; `cookies_ready` takes the pending call back.
        unsafe {
            (self.api.webkit_cookie_manager_get_cookies)(
                self.cookies,
                url.as_ptr(),
                ptr::null_mut(),
                cookies_ready,
                pending,
            );
        }
    }
}

fn clamp_extent(extent: u32) -> c_int {
    c_int::try_from(extent.clamp(1, 16_384)).unwrap_or(1)
}

/// Reads a cairo image surface into a picture.
///
/// # Safety
/// `surface` is null or a live cairo image surface.
unsafe fn read_surface(api: &Api, surface: Ptr) -> Option<Frame> {
    if surface.is_null() {
        return None;
    }
    // SAFETY: the surface is live by the caller's contract; the data is copied before it ends.
    unsafe {
        (api.cairo_surface_flush)(surface);
        let data = (api.cairo_image_surface_get_data)(surface);
        let extents = (
            usize::try_from((api.cairo_image_surface_get_width)(surface)),
            usize::try_from((api.cairo_image_surface_get_height)(surface)),
            usize::try_from((api.cairo_image_surface_get_stride)(surface)),
        );
        let (Ok(width), Ok(height), Ok(stride)) = extents else {
            return None;
        };
        if data.is_null() {
            return None;
        }
        let (mut scale_x, mut scale_y) = (1.0_f64, 1.0_f64);
        (api.cairo_surface_get_device_scale)(surface, &raw mut scale_x, &raw mut scale_y);
        let pixels = std::slice::from_raw_parts(data, stride.checked_mul(height)?);
        // A scale is a small whole number of device pixels per logical pixel.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let scale = scale_x.round().clamp(1.0, 8.0) as u32;
        Frame::from_surface(width, height, stride, scale, pixels)
    }
}

/// Makes a view and puts it in the thread's table.
pub(super) fn create(gtk: &Gtk, id: u64, options: &ViewOptions, shared: Arc<Shared>) -> Result<()> {
    let api = gtk.api;
    let (context, private_context) = match &options.profile {
        Some(profile) => (persistent_context(gtk, profile)?, None),
        None => {
            // SAFETY: a context that keeps nothing on disk.
            let context = unsafe { (api.webkit_web_context_new_ephemeral)() };
            (context, Some(context))
        }
    };
    let (width, height) = (clamp_extent(options.width), clamp_extent(options.height));
    // SAFETY: GTK was initialized on this thread; the widgets are made and joined here.
    let (webview, window, cookies) = unsafe {
        let webview = (api.webkit_web_view_new_with_context)(context);
        let window = (api.gtk_offscreen_window_new)();
        (api.gtk_window_set_default_size)(window, width, height);
        (api.gtk_widget_set_size_request)(webview, width, height);
        (api.gtk_container_add)(window, webview);
        let manager = (api.webkit_web_context_get_website_data_manager)(context);
        let cookies = (api.webkit_website_data_manager_get_cookie_manager)(manager);
        (webview, window, cookies)
    };
    if webview.is_null() || window.is_null() {
        return Err(Error::Unavailable("the web view could not be made"));
    }
    let view = Rc::new(ViewCtx {
        api,
        shared,
        webview,
        window,
        cookies,
        private_context,
        timer: Cell::new(0),
        buttons: Cell::new(0),
    });
    connect_signals(&view);
    // SAFETY: the window and its page are alive.
    unsafe { (api.gtk_widget_show_all)(window) };
    gtk.views.borrow_mut().insert(id, view);
    Ok(())
}

/// `G_SIGNAL_MATCH_DATA`: match the handlers by the data they were connected with.
const SIGNAL_MATCH_DATA: c_uint = 1 << 4;

pub(super) fn destroy(gtk: &Gtk, id: u64) {
    let Some(view) = gtk.views.borrow_mut().remove(&id) else {
        return;
    };
    // SAFETY: the timer and the window are this view's; nothing runs them after this. The page
    // can outlive the window while a call of WebKit still holds it, and a signal it emits then must
    // not find the context freed: every handler that was given it is disconnected first.
    unsafe {
        if view.timer.get() != 0 {
            (view.api.g_source_remove)(view.timer.get());
        }
        for instance in [view.webview, view.window] {
            (view.api.g_signal_handlers_disconnect_matched)(
                instance,
                SIGNAL_MATCH_DATA,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                view.data(),
            );
        }
        (view.api.gtk_widget_destroy)(view.window);
        if let Some(context) = view.private_context {
            (view.api.g_object_unref)(context);
        }
    }
}

/// The context all the views of a profile share: its own directories hold the cookies, the
/// local storage and the cache, so a login lasts across runs.
fn persistent_context(gtk: &Gtk, profile: &Path) -> Result<Ptr> {
    if let Some(context) = gtk.contexts.borrow().get(profile) {
        return Ok(*context);
    }
    let api = gtk.api;
    let data = profile.join("web");
    let cache = profile.join("web-cache");
    private_directory(&data)?;
    private_directory(&cache)?;
    let path = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::Unavailable("the web profile path is not usable"))
    };
    let (data_c, cache_c, cookies_c) = (
        path(&data)?,
        path(&cache)?,
        path(&data.join("cookies.sqlite"))?,
    );
    // SAFETY: the property names and values are zero-terminated strings that outlive the call,
    // and the list ends with a null pointer, as `g_object_new` asks.
    let context = unsafe {
        let manager = (api.object_new)(
            (api.webkit_website_data_manager_get_type)(),
            c"base-data-directory".as_ptr(),
            data_c.as_ptr(),
            c"base-cache-directory".as_ptr(),
            cache_c.as_ptr(),
            ptr::null::<c_char>(),
        );
        if manager.is_null() {
            return Err(Error::Unavailable("the web profile could not be opened"));
        }
        let context = (api.webkit_web_context_new_with_website_data_manager)(manager);
        let cookies = (api.webkit_website_data_manager_get_cookie_manager)(manager);
        (api.webkit_cookie_manager_set_persistent_storage)(
            cookies,
            cookies_c.as_ptr(),
            COOKIES_SQLITE,
        );
        context
    };
    gtk.contexts
        .borrow_mut()
        .insert(profile.to_path_buf(), context);
    Ok(context)
}

fn connect_signals(view: &Rc<ViewCtx>) {
    let data: Ptr = view.data();
    let api = view.api;
    let connect = |instance: Ptr, signal: &CStr, handler: *const ()| {
        // SAFETY: the handler has the signature the signal's documentation gives, `data` is this
        // context, which stays in the thread's table until the widgets are destroyed.
        unsafe {
            (api.g_signal_connect_data)(
                instance,
                signal.as_ptr(),
                handler.cast_mut().cast(),
                data,
                None,
                0,
            );
        }
    };
    let page = view.webview;
    connect(view.window, c"damage-event", on_damage as *const ());
    connect(page, c"load-changed", on_load_changed as *const ());
    for property in [
        c"notify::uri",
        c"notify::title",
        c"notify::estimated-load-progress",
    ] {
        connect(page, property, on_notify as *const ());
    }
    connect(page, c"decide-policy", on_decide_policy as *const ());
    connect(page, c"create", on_create as *const ());
    connect(page, c"permission-request", on_permission as *const ());
    connect(page, c"context-menu", on_context_menu as *const ());
    connect(page, c"script-dialog", on_script_dialog as *const ());
    connect(page, c"show-option-menu", on_option_menu as *const ());
    connect(page, c"run-file-chooser", on_file_chooser as *const ());
    connect(page, c"enter-fullscreen", on_fullscreen as *const ());
    connect(page, c"web-process-terminated", on_terminated as *const ());
}

/// The context a handler was connected with.
///
/// # Safety
/// `data` is the pointer given to `connect_signals`, and the view is still in the table.
unsafe fn context<'a>(data: Ptr) -> &'a ViewCtx {
    // SAFETY: by the caller's contract.
    unsafe { &*data.cast::<ViewCtx>() }
}

unsafe extern "C" fn frame_tick(data: Ptr) -> GBool {
    // SAFETY: the timer is removed before the context is dropped.
    let view = unsafe { context(data) };
    view.timer.set(0);
    view.read_live_frame();
    ffi::FALSE
}

unsafe extern "C" fn on_damage(_window: Ptr, _event: Ptr, data: Ptr) -> GBool {
    // SAFETY: the signal's handler data is this view's context.
    unsafe { context(data) }.schedule_frame();
    ffi::FALSE
}

unsafe extern "C" fn on_load_changed(_page: Ptr, event: c_int, data: Ptr) {
    // SAFETY: the signal's handler data is this view's context.
    let view = unsafe { context(data) };
    if event == LOAD_STARTED {
        let count = view.shared.navigations.fetch_add(1, Ordering::Relaxed) + 1;
        view.shared.tell(ViewUpdate::Navigating { count });
    }
    view.refresh_state();
}

unsafe extern "C" fn on_notify(_page: Ptr, _property: Ptr, data: Ptr) {
    // SAFETY: the signal's handler data is this view's context.
    unsafe { context(data) }.refresh_state();
}

/// Navigations stay inside the schemes of `policy`, and a page that is not one the engine can
/// show is not downloaded: a web pane reads pages, it does not save files.
unsafe extern "C" fn on_decide_policy(_page: Ptr, decision: Ptr, kind: c_int, data: Ptr) -> GBool {
    // SAFETY: the signal's handler data is this view's context.
    let view = unsafe { context(data) };
    let api = view.api;
    // SAFETY: the decision is alive until the handler returns.
    unsafe {
        if kind == DECISION_RESPONSE {
            if (api.webkit_response_policy_decision_is_mime_type_supported)(decision) == ffi::FALSE
            {
                (api.webkit_policy_decision_ignore)(decision);
                return ffi::TRUE;
            }
            return ffi::FALSE;
        }
        let action = (api.webkit_navigation_policy_decision_get_navigation_action)(decision);
        let request = (api.webkit_navigation_action_get_request)(action);
        let uri = copy_string((api.webkit_uri_request_get_uri)(request));
        if policy::allows(&uri) {
            return ffi::FALSE;
        }
        (api.webkit_policy_decision_ignore)(decision);
    }
    ffi::TRUE
}

/// A page that asks for a new window gets its address opened here instead: a web pane has no
/// second window to put it in.
unsafe extern "C" fn on_create(_page: Ptr, action: Ptr, data: Ptr) -> Ptr {
    // SAFETY: the signal's handler data is this view's context.
    let view = unsafe { context(data) };
    // SAFETY: the action is alive until the handler returns.
    let uri = unsafe {
        let request = (view.api.webkit_navigation_action_get_request)(action);
        copy_string((view.api.webkit_uri_request_get_uri)(request))
    };
    if policy::allows_request(&uri)
        && let Ok(target) = Target::new(crate::Navigation::Url(&uri))
    {
        view.navigate(&target);
    }
    ptr::null_mut()
}

/// A page's request for the camera, the position, notifications or the like is refused: the
/// pane asks the person nothing yet.
unsafe extern "C" fn on_permission(_page: Ptr, request: Ptr, data: Ptr) -> GBool {
    // SAFETY: the signal's handler data is this view's context; the request is alive.
    unsafe { (context(data).api.webkit_permission_request_deny)(request) };
    ffi::TRUE
}

/// There is no window to open a menu in.
unsafe extern "C" fn on_context_menu(
    _page: Ptr,
    _menu: Ptr,
    _event: Ptr,
    _hit: Ptr,
    _data: Ptr,
) -> GBool {
    ffi::TRUE
}

unsafe extern "C" fn on_option_menu(
    _page: Ptr,
    menu: Ptr,
    _event: Ptr,
    _area: Ptr,
    data: Ptr,
) -> GBool {
    // SAFETY: the signal's handler data is this view's context; the menu is alive.
    unsafe { (context(data).api.webkit_option_menu_close)(menu) };
    ffi::TRUE
}

/// An alert is read and closed, a question is answered "no", and leaving a page is allowed:
/// a dialog of the system would open a window the pane does not own.
unsafe extern "C" fn on_script_dialog(_page: Ptr, dialog: Ptr, data: Ptr) -> GBool {
    // SAFETY: the signal's handler data is this view's context; the dialog is alive.
    unsafe {
        let api = context(data).api;
        match (api.webkit_script_dialog_get_dialog_type)(dialog) {
            DIALOG_CONFIRM => (api.webkit_script_dialog_confirm_set_confirmed)(dialog, ffi::FALSE),
            DIALOG_BEFORE_UNLOAD => {
                (api.webkit_script_dialog_confirm_set_confirmed)(dialog, ffi::TRUE);
            }
            _ => {}
        }
    }
    ffi::TRUE
}

unsafe extern "C" fn on_file_chooser(_page: Ptr, request: Ptr, data: Ptr) -> GBool {
    // SAFETY: the signal's handler data is this view's context; the request is alive.
    unsafe { (context(data).api.webkit_file_chooser_request_cancel)(request) };
    ffi::TRUE
}

unsafe extern "C" fn on_fullscreen(_page: Ptr, _data: Ptr) -> GBool {
    ffi::TRUE
}

unsafe extern "C" fn on_terminated(_page: Ptr, _reason: c_int, data: Ptr) {
    // SAFETY: the signal's handler data is this view's context.
    unsafe { context(data) }.refresh_state();
}

unsafe extern "C" fn snapshot_ready(page: Ptr, result: Ptr, data: Ptr) {
    // SAFETY: `data` is the pending call `capture` made, which is answered once.
    let pending = unsafe { Pending::<Frame>::from_raw(data) };
    let api = pending.api;
    let mut error: Ptr = ptr::null_mut();
    // SAFETY: the engine's own call, with the page and result it gave this callback.
    let frame = unsafe {
        let surface = (api.webkit_web_view_get_snapshot_finish)(page, result, &raw mut error);
        let frame = read_surface(api, surface);
        if !surface.is_null() {
            (api.cairo_surface_destroy)(surface);
        }
        if !error.is_null() {
            (api.g_error_free)(error);
        }
        frame
    };
    pending.finish(frame.ok_or(Error::Page("the page could not be captured")));
}

unsafe extern "C" fn script_ready(page: Ptr, result: Ptr, data: Ptr) {
    // SAFETY: `data` is the pending call `run_script` made, which is answered once.
    let pending = unsafe { Pending::<String>::from_raw(data) };
    let api = pending.api;
    let mut error: Ptr = ptr::null_mut();
    // SAFETY: the engine's own call, with the page and result it gave this callback.
    let text = unsafe {
        let value = (api.webkit_web_view_evaluate_javascript_finish)(page, result, &raw mut error);
        let text = if value.is_null() {
            None
        } else {
            let raw = (api.jsc_value_to_string)(value);
            let text = copy_string(raw);
            (api.g_free)(raw.cast());
            (api.g_object_unref)(value);
            Some(text)
        };
        if !error.is_null() {
            (api.g_error_free)(error);
        }
        text
    };
    pending.finish(text.ok_or(Error::Page("the script did not finish")));
}

unsafe extern "C" fn cookies_ready(manager: Ptr, result: Ptr, data: Ptr) {
    // SAFETY: `data` is the pending call `cookies_present` made, which is answered once.
    let pending = unsafe { Pending::<bool>::from_raw(data) };
    let api = pending.api;
    let mut error: Ptr = ptr::null_mut();
    // SAFETY: the engine's own call; the list it returns is freed here with its own function.
    let present = unsafe {
        let list = (api.webkit_cookie_manager_get_cookies_finish)(manager, result, &raw mut error);
        let present = (api.g_list_length)(list) > 0;
        (api.g_list_free_full)(list, api.soup_cookie_free);
        let failed = !error.is_null();
        if failed {
            (api.g_error_free)(error);
        }
        present && !failed
    };
    pending.finish(Ok(present));
}
