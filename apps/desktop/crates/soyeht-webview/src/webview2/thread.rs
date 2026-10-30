//! The thread of the web view: a COM apartment of its own and a message loop, which WebView2
//! needs, and a window that only receives the jobs the rest of the program posts. A job runs in
//! the loop, so it runs again inside a wait that pumps messages; nothing here is borrowed across
//! such a wait.

use super::{host, view::ViewCtx};
use crate::{
    Error, Result,
    shared::{await_answer, refuse_environment, reply_with, start_thread},
};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicIsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    GetAvailableCoreWebView2BrowserVersionString, ICoreWebView2Environment,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::{
            Accessibility::HWINEVENTHOOK,
            WindowsAndMessaging::{
                DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
                MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, PostMessageW, QS_ALLINPUT,
                TranslateMessage, WM_APP,
            },
        },
    },
    core::{PCWSTR, PWSTR, w},
};

/// The message that says jobs are waiting.
const WM_JOBS: u32 = WM_APP + 1;

pub(super) type Job = Box<dyn FnOnce(&Ui) + Send>;

static JOBS: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
/// The window that receives the jobs, as the integer its handle is; zero before the thread starts.
static WAKE: AtomicIsize = AtomicIsize::new(0);

/// What the web view thread holds. Only that thread touches it.
pub(crate) struct Ui {
    views: RefCell<HashMap<u64, Rc<ViewCtx>>>,
    /// One environment for each user data folder, shared by its views.
    environments: RefCell<HashMap<PathBuf, ICoreWebView2Environment>>,
    /// The hook that follows each window a page sits over, by the window's handle.
    pub(super) hooks: RefCell<HashMap<isize, HWINEVENTHOOK>>,
}

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

impl Ui {
    fn new() -> Self {
        Self {
            views: RefCell::new(HashMap::new()),
            environments: RefCell::new(HashMap::new()),
            hooks: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn view(&self, id: u64) -> Option<Rc<ViewCtx>> {
        self.views.borrow().get(&id).cloned()
    }

    pub(super) fn add(&self, id: u64, view: Rc<ViewCtx>) {
        self.views.borrow_mut().insert(id, view);
    }

    pub(super) fn remove(&self, id: u64) -> Option<Rc<ViewCtx>> {
        self.views.borrow_mut().remove(&id)
    }

    pub(super) fn views(&self) -> Vec<Rc<ViewCtx>> {
        self.views.borrow().values().cloned().collect()
    }

    pub(super) fn environment(&self, folder: &PathBuf) -> Option<ICoreWebView2Environment> {
        self.environments.borrow().get(folder).cloned()
    }

    pub(super) fn keep_environment(&self, folder: PathBuf, environment: ICoreWebView2Environment) {
        self.environments.borrow_mut().insert(folder, environment);
    }
}

/// The calling thread's table, for a callback of the system that has no other way to it.
pub(super) fn current() -> Option<Rc<Ui>> {
    UI.with(|ui| ui.borrow().clone())
}

/// Starts the thread and waits until it can take jobs. A machine without the runtime fails here.
pub(super) fn start(timeout: Duration) -> Result<()> {
    // The browser can be given command-line switches from the environment of the process, among
    // them one that opens a debugging port.
    refuse_environment(
        &["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"],
        "the environment gives the web view switches of its own",
    )?;
    start_thread(timeout, |ready| serve(&ready))
}

/// Runs `job` on the web view thread, after what was posted before it.
pub(super) fn post(job: Job) {
    JOBS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push_back(job);
    let wake = HWND(WAKE.load(Ordering::Acquire) as *mut std::ffi::c_void);
    // SAFETY: a plain message to a window that lives as long as the thread; a failure means the
    // thread is gone, and the job is never run.
    if unsafe { PostMessageW(Some(wake), WM_JOBS, WPARAM(0), LPARAM(0)) }.is_err() {
        // The thread is gone: a caller that waits for the job is told by its timeout.
    }
}

/// Runs `job` on the web view thread and waits for what it sends back.
pub(super) fn ask<T: Send + 'static>(
    timeout: Duration,
    job: impl FnOnce(&Ui, mpsc::Sender<Result<T>>) + Send + 'static,
) -> Result<T> {
    let (reply, answer) = mpsc::channel();
    post(Box::new(move |ui| job(ui, reply)));
    await_answer(&answer, timeout)
}

/// Waits on the web view thread for `answer`, running the thread's messages meanwhile: a call of
/// WebView2 answers through them.
pub(super) fn wait_for<T>(answer: &mpsc::Receiver<T>, timeout: Duration) -> Result<T> {
    let end = Instant::now() + timeout;
    loop {
        match answer.try_recv() {
            Ok(value) => return Ok(value),
            Err(mpsc::TryRecvError::Disconnected) => return Err(Error::Closed),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= end {
            return Err(Error::Timeout);
        }
        pump(20);
    }
}

/// Handles the messages that are waiting, or waits up to `milliseconds` for one.
fn pump(milliseconds: u32) {
    let mut message = MSG::default();
    // SAFETY: the usual loop of a window thread, on the thread that owns its windows.
    unsafe {
        MsgWaitForMultipleObjects(None, false, milliseconds, QS_ALLINPUT);
        while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if TranslateMessage(&raw const message).as_bool() {
                // A key message that makes a character: nothing is owed for it.
            }
            DispatchMessageW(&raw const message);
        }
    }
}

fn serve(ready: &mpsc::Sender<Result<()>>) {
    match prepare() {
        Ok(()) => reply_with(ready, Ok(())),
        Err(error) => return reply_with(ready, Err(error)),
    }
    let mut message = MSG::default();
    // SAFETY: the loop of this thread, which owns the windows; a quit or an error ends it.
    unsafe {
        while GetMessageW(&raw mut message, None, 0, 0).0 > 0 {
            if TranslateMessage(&raw const message).as_bool() {
                // A key message that makes a character: nothing is owed for it.
            }
            DispatchMessageW(&raw const message);
        }
    }
}

/// The apartment, the runtime check, the classes and the window of the jobs.
fn prepare() -> Result<()> {
    // SAFETY: first call on this thread; an apartment of a thread that has one already is kept.
    let apartment = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if apartment.is_err() {
        return Err(Error::Unavailable(
            "the web view thread could not start COM",
        ));
    }
    // The runtime is asked for before anything else: a machine without it says so here.
    let mut version = PWSTR::null();
    // SAFETY: a null folder asks for the installed runtime; the string it gives is freed below.
    let found =
        unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &raw mut version) };
    found
        .map_err(|_| Error::Unavailable("the Microsoft Edge WebView2 runtime is not installed"))?;
    webview2_com::take_pwstr(version);
    super::view::remove_stale_private_folders();
    host::register_hosts()?;
    let wake = make_wake_window()?;
    WAKE.store(wake.0 as isize, Ordering::Release);
    UI.with(|ui| *ui.borrow_mut() = Some(Rc::new(Ui::new())));
    Ok(())
}

fn make_wake_window() -> Result<HWND> {
    host::register_class(w!("SoyehtNextWebJobs"), Some(wake_proc))?;
    host::create_window(
        w!("SoyehtNextWebJobs"),
        Default::default(),
        Default::default(),
        (0, 0),
        Some(HWND_MESSAGE),
    )
}

unsafe extern "system" fn wake_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_JOBS {
        run_jobs();
        return LRESULT(0);
    }
    // SAFETY: the default procedure of the system, with this call's own arguments.
    unsafe { DefWindowProcW(window, message, w, l) }
}

/// Runs every waiting job. The queue is not held while one runs: a job may wait, and post.
fn run_jobs() {
    let Some(ui) = current() else {
        return;
    };
    loop {
        let job = JOBS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        match job {
            Some(job) => job(&ui),
            None => return,
        }
    }
}
