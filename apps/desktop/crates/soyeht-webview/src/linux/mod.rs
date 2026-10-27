//! The WebKitGTK 4.1 back end. GTK is not thread safe and wants a main loop, so one thread of its
//! own owns every GTK object and runs that loop; the rest of the program talks to it through
//! jobs and gets pictures, state and answers back. Nothing here draws on the screen: the page is
//! rendered into an off-screen window and read out as pixels.

mod calls;
mod ffi;
mod input;
mod view;

use crate::{
    Error, Frame, KeyInput, Modifiers, Navigation, PageState, Pointer, Result, ScrollInput,
    ViewOptions, ViewSink,
};
use ffi::{Api, Ptr};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_int,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use view::{Shared, ViewCtx};

/// How long the first view waits for the GTK thread to start.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a script waits for an answer.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a capture of the page waits for an answer.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(45);

type Job = Box<dyn FnOnce(&Gtk) + Send>;

/// What the GTK thread holds. Only that thread touches it.
pub(crate) struct Gtk {
    api: &'static Api,
    /// One persistent context for each profile directory, shared by its views.
    contexts: RefCell<HashMap<PathBuf, Ptr>>,
    views: RefCell<HashMap<u64, Rc<ViewCtx>>>,
    pointer: Cell<Ptr>,
    keyboard: Cell<Ptr>,
    keymap: Cell<Ptr>,
}

thread_local! {
    static GTK: RefCell<Option<Rc<Gtk>>> = const { RefCell::new(None) };
}

/// The process's web view service: the thread that runs GTK.
pub struct Backend {
    api: &'static Api,
    next_view: AtomicU64,
}

static BACKEND: OnceLock<std::result::Result<Backend, Error>> = OnceLock::new();

impl Backend {
    /// The service, started on first use. A machine without the system web view, or without a
    /// display, gets the same error on every call.
    pub fn get() -> Result<&'static Self> {
        BACKEND
            .get_or_init(Self::start)
            .as_ref()
            .map_err(Clone::clone)
    }

    fn start() -> Result<Self> {
        refuse_debugging_environment()?;
        let (ready, started) = mpsc::channel();
        std::thread::Builder::new()
            .name("soyeht-webview".into())
            .spawn(move || serve(&ready))
            .map_err(|_| Error::Unavailable("the web view thread did not start"))?;
        match started.recv_timeout(START_TIMEOUT) {
            Ok(Ok(api)) => Ok(Self {
                api,
                next_view: AtomicU64::new(1),
            }),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::Timeout)
            }
        }
    }

    /// Runs `job` on the GTK thread, after what was posted before it.
    fn post(&self, job: Job) {
        let raw = Box::into_raw(Box::new(job)).cast();
        // SAFETY: `run_job` takes back the box made above, once; the main context runs it.
        unsafe {
            (self.api.g_idle_add_full)(ffi::PRIORITY_DEFAULT, run_job, raw, None);
        }
    }

    /// Runs `job` on the GTK thread and waits for what it sends back.
    fn ask<T: Send + 'static>(
        &self,
        timeout: Duration,
        job: impl FnOnce(&Gtk, mpsc::Sender<Result<T>>) + Send + 'static,
    ) -> Result<T> {
        let (reply, answer) = mpsc::channel();
        self.post(Box::new(move |gtk| job(gtk, reply)));
        answer.recv_timeout(timeout).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => Error::Timeout,
            mpsc::RecvTimeoutError::Disconnected => Error::Closed,
        })?
    }

    /// Opens a view. `sink` hears of pictures, state and navigations from the GTK thread.
    pub fn open(&'static self, options: &ViewOptions, sink: ViewSink) -> Result<WebView> {
        let id = self.next_view.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(Shared::new(sink));
        let options = options.clone();
        let for_view = Arc::clone(&shared);
        self.ask(START_TIMEOUT, move |gtk, reply| {
            reply_with(&reply, view::create(gtk, id, &options, for_view));
        })?;
        Ok(WebView {
            id,
            backend: self,
            shared,
        })
    }
}

fn reply_with<T>(reply: &mpsc::Sender<Result<T>>, value: Result<T>) {
    if reply.send(value).is_err() {
        // The caller timed out and left; there is nobody to tell.
    }
}

/// A site's inspector can be opened from the environment of the process (an inspector server
/// that listens on a port). A web pane never runs with one: it would be a way into the page.
fn refuse_debugging_environment() -> Result<()> {
    for name in [
        concat!("WEBKIT", "_INSPECTOR_SERVER"),
        concat!("WEBKIT", "_INSPECTOR_HTTP_SERVER"),
    ] {
        if std::env::var_os(name).is_some() {
            return Err(Error::Unavailable(
                "the environment turns on a debugging server of the web view",
            ));
        }
    }
    Ok(())
}

/// The body of the GTK thread.
fn serve(ready: &mpsc::Sender<Result<&'static Api>>) {
    let api: &'static Api = match Api::load() {
        Ok(api) => Box::leak(Box::new(api)),
        Err(error) => return reply_with(ready, Err(error)),
    };
    let mut argc: c_int = 0;
    // SAFETY: GTK is initialized once, on this thread, with no arguments.
    let displayed = unsafe { (api.gtk_init_check)(&raw mut argc, std::ptr::null_mut()) };
    if displayed == ffi::FALSE {
        return reply_with(
            ready,
            Err(Error::Unavailable("there is no display for the web view")),
        );
    }
    GTK.with(|cell| *cell.borrow_mut() = Some(Rc::new(Gtk::new(api))));
    reply_with(ready, Ok(api));
    // SAFETY: a main loop of the default context, run on the thread that owns GTK.
    unsafe {
        let main_loop = (api.g_main_loop_new)(std::ptr::null_mut(), ffi::FALSE);
        (api.g_main_loop_run)(main_loop);
    }
}

unsafe extern "C" fn run_job(raw: Ptr) -> ffi::GBool {
    // SAFETY: `raw` is the box `post` made for this call, which runs once.
    let job = *unsafe { Box::from_raw(raw.cast::<Job>()) };
    let gtk = GTK.with(|cell| cell.borrow().clone());
    if let Some(gtk) = gtk {
        job(&gtk);
    }
    ffi::FALSE
}

impl Gtk {
    pub(crate) fn view(&self, id: u64) -> Option<Rc<ViewCtx>> {
        self.views.borrow().get(&id).cloned()
    }
}

/// One page in one pane.
pub struct WebView {
    id: u64,
    backend: &'static Backend,
    shared: Arc<Shared>,
}

impl WebView {
    fn with_view(&self, work: impl FnOnce(&Gtk, &ViewCtx) + Send + 'static) {
        let id = self.id;
        self.backend.post(Box::new(move |gtk| {
            if let Some(view) = gtk.view(id) {
                work(gtk, &view);
            }
        }));
    }

    fn ask_view<T: Send + 'static>(
        &self,
        timeout: Duration,
        work: impl FnOnce(&Gtk, &ViewCtx, mpsc::Sender<Result<T>>) + Send + 'static,
    ) -> Result<T> {
        let id = self.id;
        self.backend
            .ask(timeout, move |gtk, reply| match gtk.view(id) {
                Some(view) => work(gtk, &view, reply),
                None => reply_with(&reply, Err(Error::Closed)),
            })
    }

    /// Loads an address, or goes back, forward or again.
    pub fn navigate(&self, navigation: Navigation<'_>) -> Result<()> {
        let target = calls::Target::new(navigation)?;
        self.with_view(move |_, view| view.navigate(&target));
        Ok(())
    }

    /// The size the page lays out for, in logical pixels.
    pub fn resize(&self, width: u32, height: u32) {
        self.with_view(move |_, view| view.resize(width, height));
    }

    pub fn pointer(&self, pointer: Pointer, modifiers: Modifiers) {
        self.with_view(move |gtk, view| input::pointer(gtk, view, pointer, modifiers));
    }

    pub fn scroll(&self, scroll: ScrollInput, modifiers: Modifiers) {
        self.with_view(move |gtk, view| input::scroll(gtk, view, scroll, modifiers));
    }

    pub fn key(&self, key: KeyInput) {
        self.with_view(move |gtk, view| input::key(gtk, view, key));
    }

    /// The pane has or has lost the keyboard focus.
    pub fn focus(&self, focused: bool) {
        self.with_view(move |gtk, view| input::focus(gtk, view, focused));
    }

    /// Text the person composed or pasted, as the page's own editing takes it.
    pub fn insert_text(&self, text: &str) {
        let text = calls::c_string(text);
        self.with_view(move |_, view| view.edit_with(c"InsertText", text.as_deref()));
    }

    pub fn select_all(&self) {
        self.with_view(|_, view| view.edit_with(c"SelectAll", None));
    }

    pub fn delete_selection(&self) {
        self.with_view(|_, view| view.edit_with(c"Delete", None));
    }

    /// The latest picture the page drew, once; `None` when it has drawn nothing since.
    pub fn take_frame(&self) -> Option<Frame> {
        self.shared
            .frame
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    pub fn state(&self) -> PageState {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many navigations the view has begun.
    pub fn navigations(&self) -> u64 {
        self.shared.navigations.load(Ordering::Relaxed)
    }

    /// Takes a picture of what the pane shows, as the engine itself draws it.
    pub fn capture(&self) -> Result<Frame> {
        self.ask_view(CAPTURE_TIMEOUT, move |_, view, reply| {
            view.capture(reply);
        })
    }

    /// Runs a script in the engine's isolated world, where the page cannot see it, and returns
    /// what it returns as text.
    pub fn run_script(&self, source: &str) -> Result<String> {
        let source = calls::c_string(source).ok_or(Error::Page("the script is not text"))?;
        self.ask_view(SCRIPT_TIMEOUT, move |_, view, reply| {
            view.run_script(&source, reply);
        })
    }

    /// Whether the profile holds a cookie for this address.
    pub fn cookies_present(&self, url: &str) -> Result<bool> {
        let url = calls::c_string(url).ok_or(Error::Page("the address is not text"))?;
        self.ask_view(SCRIPT_TIMEOUT, move |_, view, reply| {
            view.cookies_present(&url, reply);
        })
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        let id = self.id;
        self.backend
            .post(Box::new(move |gtk| view::destroy(gtk, id)));
    }
}

/// The profile's directories, made private to the person.
pub(crate) fn private_directory(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| Error::Unavailable("the web profile directory cannot be made"))
}

const _: fn() = || {
    fn is_send_and_sync<T: Send + Sync>() {}
    is_send_and_sync::<WebView>();
};
