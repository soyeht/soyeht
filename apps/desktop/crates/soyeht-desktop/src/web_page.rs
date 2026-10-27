//! The pages of the window's web panes: one system web view for each pane the engine holds, made
//! when the pane first shows up and dropped when it closes. The window draws them, and the
//! engine's agents read them through the host of `soyeht_client::web_host`, which finds them
//! here by pane (ADR 0110).

use soyeht_client::web_host::{Handoff, HostError, Page, PageInfo, Pages, Picture, Target};
use soyeht_protocol::Pane;
use soyeht_webview::{
    Backend, Error, Frame, Modifiers, Navigation, PageState, ScrollInput, ViewOptions, ViewSink,
    ViewUpdate, WebView,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

/// The size a page lays out for before its pane has been drawn once.
const FIRST_SIZE: (u32, u32) = (1024, 768);

/// What a page tells the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signal {
    /// The page for this pane is ready to draw.
    Created(String),
    /// A new picture of the page is ready.
    Frame(String),
    /// The page's address, title or loading changed.
    State(String),
}

/// One web pane's page.
pub(crate) struct WebPage {
    view: WebView,
    incognito: bool,
    /// The pane's stored generation when the page was made: navigations count from there.
    base_generation: u64,
    /// Who has the pane, the person or an agent.
    handoff: Handoff,
    /// A picture is ready and the window has not taken it yet; another is not announced meanwhile.
    frame_ready: Arc<AtomicBool>,
    /// What the engine was last told about the page, so a page that changes only in loading is
    /// not reported again.
    reported: Mutex<(String, String)>,
}

impl WebPage {
    pub(crate) fn state(&self) -> PageState {
        self.view.state()
    }

    /// The picture the page last drew, once.
    pub(crate) fn take_frame(&self) -> Option<Frame> {
        self.frame_ready.store(false, Ordering::Release);
        self.view.take_frame()
    }

    pub(crate) fn view(&self) -> &WebView {
        &self.view
    }

    pub(crate) fn control(&self) -> &Handoff {
        &self.handoff
    }

    /// The address and title that were not yet reported to the engine, and the generation to
    /// report them under, once the page has loaded something the engine can hold.
    pub(crate) fn unreported(&self) -> Option<(u64, String, String)> {
        let state = self.view.state();
        let is_web = state.url.starts_with("http://") || state.url.starts_with("https://");
        let mut reported = self.reported.lock().unwrap_or_else(PoisonError::into_inner);
        if state.loading || !is_web || *reported == (state.url.clone(), state.title.clone()) {
            return None;
        }
        *reported = (state.url.clone(), state.title.clone());
        Some((Page::generation(self), state.url, state.title))
    }
}

impl Page for WebPage {
    fn incognito(&self) -> bool {
        self.incognito
    }

    fn navigate(&self, target: &Target) -> Result<(), HostError> {
        let navigation = match target {
            Target::Url(url) => Navigation::Url(url),
            Target::Back => Navigation::Back,
            Target::Forward => Navigation::Forward,
            Target::Reload => Navigation::Reload,
            Target::ReloadIgnoringCache => Navigation::ReloadIgnoringCache,
        };
        self.view
            .navigate(navigation)
            .map_err(|_| HostError::NavigationRefused("Only http and https addresses open."))
    }

    fn info(&self) -> PageInfo {
        let state = self.view.state();
        PageInfo {
            url: state.url,
            title: state.title,
            loading: state.loading,
        }
    }

    fn generation(&self) -> u64 {
        self.base_generation + self.view.navigations()
    }

    fn run_script(&self, source: &str) -> Result<String, HostError> {
        self.view
            .run_script(source)
            .map_err(|_| HostError::Internal("The page could not be read."))
    }

    fn picture(&self) -> Result<Picture, HostError> {
        let frame = self
            .view
            .capture()
            .map_err(|_| HostError::Internal("The page snapshot failed."))?;
        Ok(Picture {
            width: frame.width,
            height: frame.height,
            stride: frame.stride,
            bytes: frame.bytes,
        })
    }

    fn wheel(&self, x: f64, y: f64, lines: f64) {
        self.view.scroll(
            ScrollInput {
                x,
                y,
                dx: 0.0,
                dy: lines,
            },
            Modifiers::default(),
        );
    }

    fn handoff(&self) -> &Handoff {
        &self.handoff
    }

    fn has_cookies(&self, url: &str) -> Result<bool, HostError> {
        self.view
            .cookies_present(url)
            .map_err(|_| HostError::Internal("The page's cookies could not be read."))
    }
}

/// The pages of every web pane, shared by the window and the host thread.
pub(crate) struct WebRegistry {
    /// Where the profile's cookies, local storage and cache are kept.
    profile: PathBuf,
    pages: Mutex<HashMap<String, Arc<WebPage>>>,
    /// Panes whose page is being made.
    making: Mutex<HashSet<String>>,
    signals: async_channel::Sender<Signal>,
    /// Why there is no web view, when there is none.
    unavailable: Mutex<Option<Error>>,
}

impl WebRegistry {
    pub(crate) fn new(profile: PathBuf, signals: async_channel::Sender<Signal>) -> Arc<Self> {
        Arc::new(Self {
            profile,
            pages: Mutex::new(HashMap::new()),
            making: Mutex::new(HashSet::new()),
            signals,
            unavailable: Mutex::new(None),
        })
    }

    pub(crate) fn get(&self, pane_id: &str) -> Option<Arc<WebPage>> {
        self.pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(pane_id)
            .cloned()
    }

    /// Why the system web view is not available, once a page has failed to open.
    pub(crate) fn unavailable(&self) -> Option<Error> {
        self.unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Makes the page of every web pane that has none, and drops the pages of panes that are
    /// gone. A page is made off the interface thread: the first one loads the system library.
    pub(crate) fn sync<'a>(self: &Arc<Self>, panes: impl Iterator<Item = &'a Pane>) {
        let live: Vec<&Pane> = panes.collect();
        self.pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|id, _| live.iter().any(|pane| &pane.pane_id == id));
        if self.unavailable().is_some() {
            return;
        }
        for pane in live {
            let first_time = self
                .making
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(pane.pane_id.clone());
            if !first_time || self.get(&pane.pane_id).is_some() {
                continue;
            }
            let (registry, pane) = (Arc::clone(self), pane.clone());
            std::thread::spawn(move || registry.make(&pane));
        }
    }

    fn make(&self, pane: &Pane) {
        let id = pane.pane_id.clone();
        let sink = self.sink(&id);
        let frame_ready = Arc::new(AtomicBool::new(false));
        let announced = Arc::clone(&frame_ready);
        let sink: ViewSink = Arc::new(move |update| match update {
            ViewUpdate::Frame => {
                // One announcement for as many pictures as the window has not taken yet.
                if !announced.swap(true, Ordering::AcqRel) {
                    sink(update);
                }
            }
            other => sink(other),
        });
        let incognito = pane.incognito == Some(true);
        let opened = Backend::get().and_then(|backend| {
            backend.open(
                &ViewOptions {
                    profile: (!incognito).then(|| self.profile.clone()),
                    width: FIRST_SIZE.0,
                    height: FIRST_SIZE.1,
                },
                sink,
            )
        });
        match opened {
            Ok(view) => {
                let page = Arc::new(WebPage {
                    view,
                    incognito,
                    base_generation: u64::from(pane.generation),
                    handoff: Handoff::default(),
                    frame_ready,
                    reported: Mutex::new((String::new(), String::new())),
                });
                if let Some(url) = &pane.url {
                    // A pane the engine already holds opens on the page it was on. An address
                    // the web view refuses leaves the pane empty.
                    if page.view.navigate(Navigation::Url(url)).is_err() {
                        soyeht_platform::log::event("desktop.web_pane_address_refused", &[]);
                    }
                }
                self.pages
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(id.clone(), page);
                self.tell(Signal::Created(id));
            }
            Err(error) => {
                soyeht_platform::log::event("desktop.web_view_unavailable", &[]);
                *self
                    .unavailable
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(error);
                self.tell(Signal::Created(id));
            }
        }
    }

    /// What a page of `pane_id` reports, as a signal for the window.
    fn sink(&self, pane_id: &str) -> impl Fn(ViewUpdate) + Send + Sync + 'static {
        let (signals, pane_id) = (self.signals.clone(), pane_id.to_owned());
        move |update| {
            let signal = match update {
                ViewUpdate::Frame => Signal::Frame(pane_id.clone()),
                ViewUpdate::State | ViewUpdate::Navigating { .. } => Signal::State(pane_id.clone()),
            };
            if signals.try_send(signal).is_err() {
                // The window is closing; nobody is left to draw.
            }
        }
    }

    fn tell(&self, signal: Signal) {
        if self.signals.try_send(signal).is_err() {
            // The window is closing; nobody is left to draw.
        }
    }
}

impl Pages for WebRegistry {
    fn page(&self, pane_id: &str) -> Option<Arc<dyn Page>> {
        self.get(pane_id).map(|page| page as Arc<dyn Page>)
    }
}
