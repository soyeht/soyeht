//! The WebView2 back end (ADR 0124). The system's own web view (Microsoft Edge's engine, which
//! Windows 11 ships) runs the page; one thread of this crate, with a message loop, owns every COM
//! object and the windows that hold the pages, and the rest of the program talks to it through
//! jobs and gets state and answers back. A page is a window of its own over the pane, placed by
//! [`WebView::place`], that takes the person's pointer and keys itself.

mod capture;
mod handlers;
mod host;
mod overrides;
mod private;
mod script;
mod thread;
mod view;
mod wide;

use crate::shared::{
    CAPTURE_TIMEOUT, SCRIPT_TIMEOUT, START_TIMEOUT, Shared, ask_found, new_view, reply_with,
    shared_view_calls,
};
use crate::{
    Error, Frame, KeyInput, Modifiers, Navigation, Placement, Pointer, Result, ScrollInput,
    ViewOptions, ViewSink,
};
use std::{
    rc::Rc,
    sync::{Arc, Mutex, PoisonError, atomic::AtomicU64, mpsc},
    time::Duration,
};
use thread::Ui;
use view::ViewCtx;

/// The process's web view service: the thread that runs WebView2.
pub struct Backend {
    next_view: AtomicU64,
}

/// What the first start said, when it said it for good: a machine without the runtime, or with
/// an override of it, gets the same answer on every call. A start that only ran out of time is
/// not kept, and is asked for again.
static BACKEND: Mutex<Option<Result<&'static Backend>>> = Mutex::new(None);

impl Backend {
    /// The service, started on first use.
    pub fn get() -> Result<&'static Self> {
        let mut kept = BACKEND.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(answer) = kept.as_ref() {
            return answer.clone();
        }
        let answer = match thread::start(START_TIMEOUT) {
            Ok(()) => Ok(&*Box::leak(Box::new(Self {
                next_view: AtomicU64::new(1),
            }))),
            Err(error) if matches!(error, Error::Unavailable(_) | Error::Unsupported) => Err(error),
            Err(error) => return Err(error),
        };
        *kept = Some(answer.clone());
        answer
    }

    /// Opens a view. `sink` hears of the state and the navigations from the web view thread.
    pub fn open(&'static self, options: &ViewOptions, sink: ViewSink) -> Result<WebView> {
        let (id, shared) = new_view(&self.next_view, sink);
        let options = options.clone();
        let for_view = Arc::clone(&shared);
        // The wait covers the two steps of a page's making, each with its own deadline.
        thread::ask(view::OPEN_TIMEOUT, move |ui, reply| {
            view::create(ui, id, &options, for_view, &reply);
        })?;
        Ok(WebView { id, shared })
    }
}

/// One page in one pane.
pub struct WebView {
    id: u64,
    shared: Arc<Shared>,
}

impl WebView {
    fn with_view(&self, work: impl FnOnce(&Ui, &ViewCtx) + Send + 'static) {
        let id = self.id;
        thread::post(Box::new(move |ui| {
            if let Some(view) = ui.view(id) {
                work(ui, &view);
            }
        }));
    }

    fn ask_view<T: Send + 'static>(
        &self,
        timeout: Duration,
        work: impl FnOnce(&Ui, &Rc<ViewCtx>, mpsc::Sender<Result<T>>) + Send + 'static,
    ) -> Result<T> {
        let id = self.id;
        thread::ask(timeout, move |ui, reply| {
            ask_found(ui, ui.view(id), reply, work);
        })
    }

    /// Loads an address, or goes back, forward or again.
    pub fn navigate(&self, navigation: Navigation<'_>) -> Result<()> {
        let target = view::target(navigation)?;
        self.with_view(move |_, view| view.navigate(&target));
        Ok(())
    }

    /// The pane's size: where the page is, once it is placed, says it better.
    pub fn resize(&self, width: u32, height: u32) {
        self.with_view(move |_, view| view.resize(width, height));
    }

    /// Puts the page over its pane and shows or hides it.
    pub fn place(&self, placement: Placement) {
        self.with_view(move |ui, view| view.place(ui, &placement));
    }

    /// The person's pointer is the page's own: it arrives at the page's window.
    pub fn pointer(&self, _: Pointer, _: Modifiers) {}

    /// One scroll of the wheel at a point of the page, as the system delivers a wheel's.
    pub fn scroll(&self, scroll: ScrollInput, _: Modifiers) {
        self.with_view(move |_, view| view.wheel(&scroll));
    }

    /// The person's keys are the page's own: they arrive at the page's window.
    pub fn key(&self, _: KeyInput) {}

    /// The pane has or has lost the keyboard focus.
    pub fn focus(&self, focused: bool) {
        self.with_view(move |_, view| view.focus(focused));
    }

    /// Text typed or pasted reaches the page's own window.
    pub fn insert_text(&self, _: &str) {}

    pub fn select_all(&self) {}

    pub fn delete_selection(&self) {}

    shared_view_calls!();

    /// Takes a picture of what the pane shows, as the engine itself draws it.
    pub fn capture(&self) -> Result<Frame> {
        let (png, scale) = self.ask_view(CAPTURE_TIMEOUT, move |_, view, reply| {
            capture::capture(view, reply);
        })?;
        // The PNG is read into pixels here, off the thread that serves every page.
        Frame::from_png(&png, scale).ok_or(Error::Page("the picture could not be read"))
    }

    /// Runs a script in a world of the engine where the page cannot see it, and returns what it
    /// returns as text.
    pub fn run_script(&self, source: &str) -> Result<String> {
        let source = source.to_owned();
        self.ask_view(SCRIPT_TIMEOUT, move |_, view, reply| {
            script::run(view, &source, reply);
        })
    }

    /// Whether the profile holds a cookie for this address.
    pub fn cookies_present(&self, url: &str) -> Result<bool> {
        let url = url.to_owned();
        self.ask_view(SCRIPT_TIMEOUT, move |_, view, reply| {
            script::cookies_present(view, &url, reply);
        })
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        let id = self.id;
        thread::post(Box::new(move |ui| view::destroy(ui, id)));
    }
}

const _: fn() = || {
    fn is_send_and_sync<T: Send + Sync>() {}
    is_send_and_sync::<WebView>();
};
