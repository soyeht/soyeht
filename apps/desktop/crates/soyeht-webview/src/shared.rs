//! What every back end does the same way: what a view shares with the threads that hold it, the
//! waits for an answer that comes later, and the start of the thread that serves the views.

use crate::{Error, Frame, Navigation, PageState, Result, ViewSink, ViewUpdate, policy};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;

/// How long the first view waits for the thread that serves the views to start, and a view for
/// its page.
pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a script waits for an answer.
pub(crate) const SCRIPT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a capture of the page waits for an answer.
pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_secs(45);

/// What the other threads see of a view.
pub(crate) struct Shared {
    /// The latest picture, where the page is drawn off screen; it stays empty where the page is a
    /// window of its own.
    frame: Mutex<Option<Frame>>,
    state: Mutex<PageState>,
    navigations: AtomicU64,
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

    pub fn tell(&self, update: ViewUpdate) {
        (self.sink)(update);
    }

    /// The latest picture, once; `None` when the page has drawn nothing since.
    pub fn take_frame(&self) -> Option<Frame> {
        self.frame
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Keeps a picture the page drew and tells the window.
    #[cfg(target_os = "linux")]
    pub fn put_frame(&self, frame: Frame) {
        *self.frame.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
        self.tell(ViewUpdate::Frame);
    }

    pub fn state(&self) -> PageState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Keeps `state` and tells the window, when it is not what the window already has.
    pub fn set_state(&self, state: PageState) {
        let mut current = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if *current != state {
            *current = state;
            drop(current);
            self.tell(ViewUpdate::State);
        }
    }

    /// How many navigations the view has begun.
    pub fn navigations(&self) -> u64 {
        self.navigations.load(Ordering::Relaxed)
    }

    /// A navigation began: counts it and tells the window.
    pub fn navigation_began(&self) {
        let count = self.navigations.fetch_add(1, Ordering::Relaxed) + 1;
        self.tell(ViewUpdate::Navigating { count });
    }
}

/// The next number of a view and what it shares, for a view `sink` hears of.
pub(crate) fn new_view(counter: &AtomicU64, sink: ViewSink) -> (u64, Arc<Shared>) {
    (
        counter.fetch_add(1, Ordering::Relaxed),
        Arc::new(Shared::new(sink)),
    )
}

/// Does `work` on the view that was looked up, or answers that there is none any more.
pub(crate) fn ask_found<Table, View, T>(
    table: &Table,
    found: Option<View>,
    reply: mpsc::Sender<Result<T>>,
    work: impl FnOnce(&Table, &View, mpsc::Sender<Result<T>>),
) {
    match found {
        Some(view) => work(table, &view, reply),
        None => reply_with(&reply, Err(Error::Closed)),
    }
}

/// The calls of a view that read what it shares: the facade of each back end has them as they are.
#[cfg_attr(not(target_os = "linux"), allow(unused_macros))]
macro_rules! shared_view_calls {
    () => {
        /// The latest picture the page drew, once; `None` when it has drawn nothing since.
        pub fn take_frame(&self) -> Option<$crate::Frame> {
            self.shared.take_frame()
        }

        pub fn state(&self) -> $crate::PageState {
            self.shared.state()
        }

        /// How many navigations the view has begun.
        pub fn navigations(&self) -> u64 {
            self.shared.navigations()
        }
    };
}
#[cfg_attr(not(target_os = "linux"), allow(unused_imports))]
pub(crate) use shared_view_calls;

/// An answer to a request that the thread of the views makes, or does not.
pub(crate) fn reply_with<T>(reply: &mpsc::Sender<Result<T>>, value: Result<T>) {
    if reply.send(value).is_err() {
        // The caller timed out and left; there is nobody to tell.
    }
}

/// Waits for what the thread of the views sends back.
pub(crate) fn await_answer<T>(answer: &mpsc::Receiver<Result<T>>, timeout: Duration) -> Result<T> {
    answer.recv_timeout(timeout).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => Error::Timeout,
        mpsc::RecvTimeoutError::Disconnected => Error::Closed,
    })?
}

/// Starts the thread of the views and waits until it says it can take them.
pub(crate) fn start_thread<T: Send + 'static>(
    timeout: Duration,
    serve: impl FnOnce(mpsc::Sender<Result<T>>) + Send + 'static,
) -> Result<T> {
    let (ready, started) = mpsc::channel();
    std::thread::Builder::new()
        .name("soyeht-webview".into())
        .spawn(move || serve(ready))
        .map_err(|_| Error::Unavailable("the web view thread did not start"))?;
    match started.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
            Err(Error::Timeout)
        }
    }
}

/// The web view can be given a debugging server or switches from the environment of the process.
/// A web pane never runs with one: it would be a way into the page.
pub(crate) fn refuse_environment(names: &[&str], why: &'static str) -> Result<()> {
    if names.iter().any(|name| std::env::var_os(name).is_some()) {
        return Err(Error::Unavailable(why));
    }
    Ok(())
}

/// What a navigation loads, with the address in the form the web view takes.
pub(crate) enum Target<Address> {
    Url(Address),
    Back,
    Forward,
    Reload,
    ReloadIgnoringCache,
}

impl<Address> Target<Address> {
    /// `address` makes the web view's form of an address, which cannot hold every text.
    pub fn new(
        navigation: Navigation<'_>,
        address: impl FnOnce(&str) -> Option<Address>,
    ) -> Result<Self> {
        Ok(match navigation {
            Navigation::Url(url) => {
                if !policy::allows_request(url) {
                    return Err(Error::Page("only http and https addresses open"));
                }
                Self::Url(address(url).ok_or(Error::Page("the address is not text"))?)
            }
            Navigation::Back => Self::Back,
            Navigation::Forward => Self::Forward,
            Navigation::Reload => Self::Reload,
            Navigation::ReloadIgnoringCache => Self::ReloadIgnoringCache,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_state_that_changed_is_told_once_and_a_navigation_is_counted() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        let shared = Shared::new(Arc::new(move |update| {
            sink.lock().unwrap().push(update);
        }));
        let state = PageState {
            url: "https://example.com/".into(),
            ..PageState::default()
        };
        shared.set_state(state.clone());
        shared.set_state(state.clone());
        shared.navigation_began();
        assert_eq!(shared.state(), state);
        assert_eq!(shared.navigations(), 1);
        assert_eq!(
            *heard.lock().unwrap(),
            [ViewUpdate::State, ViewUpdate::Navigating { count: 1 }]
        );
    }

    #[test]
    fn an_address_that_is_not_a_web_address_or_not_text_is_refused() {
        let text = |url: &str| Some(url.to_owned());
        assert!(Target::new(Navigation::Url("https://example.com/"), text).is_ok());
        assert!(Target::new(Navigation::Url("file:///etc/passwd"), text).is_err());
        assert!(Target::new(Navigation::Url("https://a.b/"), |_| None::<String>).is_err());
        assert!(matches!(
            Target::<String>::new(Navigation::Back, text),
            Ok(Target::Back)
        ));
    }

    #[test]
    fn an_answer_that_never_comes_is_a_timeout_and_a_gone_thread_is_closed() {
        let (reply, answer) = mpsc::channel::<Result<u8>>();
        assert_eq!(
            await_answer(&answer, Duration::from_millis(5)),
            Err(Error::Timeout)
        );
        reply_with(&reply, Ok(3));
        assert_eq!(await_answer(&answer, Duration::from_millis(5)), Ok(3));
        drop(reply);
        assert_eq!(
            await_answer(&answer, Duration::from_millis(5)),
            Err(Error::Closed)
        );
    }
}
