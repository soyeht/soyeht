//! The pages of the window's web panes: one system web view for each pane the engine holds, made
//! when the pane first shows up and dropped when it closes. The window draws them, and the
//! engine's agents read them through the host of `soyeht_client::web_host`, which finds them
//! here by pane (ADR 0110).

use soyeht_client::web_host::{Handoff, HostError, Page, PageInfo, Pages, Picture, Target};
use soyeht_protocol::{Pane, web::same_origin};
use soyeht_webview::{
    Backend, Error, Frame, Modifiers, Navigation, PageLimit, PageState, Placement, ScrollInput,
    ViewOptions, ViewSink, ViewUpdate, WebView,
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
    handoff: Arc<Handoff>,
    /// A picture is ready and the window has not taken it yet; another is not announced meanwhile.
    frame_ready: Arc<AtomicBool>,
    /// What the engine was last told about the page, so a page that changes only in loading is
    /// not reported again.
    reported: Mutex<(String, String)>,
    /// Where the page was last placed over its pane, when the system's page is a window of its own.
    placed: Mutex<Option<Placement>>,
    /// The address of the pane that the page was last sent to, or found to be at, while the pane
    /// shows a project file: a sync that sees the same address does not send the page again.
    sent: Mutex<Option<String>>,
}

/// The origin a pane that shows a project file as a page may not leave: the address the engine
/// holds for it (contract 11, section 5b). An ordinary pane has none.
fn limit_of(pane: &Pane) -> Option<PageLimit> {
    pane.project_path.as_ref()?;
    pane.url
        .as_deref()
        .map(|url| PageLimit::new(url, same_origin))
}

/// Whether the engine has served a project page again, at an address with another origin than the
/// one the page is at: it did not already send the page there. A page that has not committed yet
/// (`current` empty) is on its way to what it was sent.
fn served_again(url: &str, current: &str, sent: Option<&str>) -> bool {
    sent != Some(url) && !same_origin(url, current)
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

    /// Puts the page over its pane, when it is a window of its own. The same place is not sent
    /// twice.
    pub(crate) fn place(&self, placement: Placement) {
        let mut placed = self.placed.lock().unwrap_or_else(PoisonError::into_inner);
        if *placed == Some(placement) {
            return;
        }
        *placed = Some(placement);
        drop(placed);
        self.view.place(placement);
    }

    /// Takes the page off the screen, where it was, until it is placed again.
    pub(crate) fn hide(&self) {
        let mut placed = self.placed.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(placement) = placed.as_mut()
            && placement.visible
        {
            placement.visible = false;
            let hidden = *placement;
            drop(placed);
            self.view.place(hidden);
        }
    }

    /// Sends the page to `url` and remembers that it went there.
    fn go(&self, url: &str) {
        *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = Some(url.to_owned());
        // A pane the engine already holds opens on the page it was on. An address the web view
        // refuses leaves the pane empty.
        if self.view.navigate(Navigation::Url(url)).is_err() {
            soyeht_platform::log::event("desktop.web_pane_address_refused", &[]);
        }
    }

    /// Keeps the page in step with its pane record, at every sync. A pane that shows a project
    /// file holds the page to the origin of the address the engine holds; when the engine served
    /// the file again (it restarted, at another port and token) the page goes to the new address,
    /// which is the new limit.
    pub(crate) fn align(&self, pane: &Pane) {
        let limit = limit_of(pane);
        let limited = limit.is_some();
        self.view.limit_to_page(limit);
        let Some(url) = pane.url.as_deref().filter(|_| limited) else {
            return;
        };
        let moved = {
            let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
            let moved = served_again(url, &self.view.state().url, sent.as_deref());
            *sent = Some(url.to_owned());
            moved
        };
        if moved {
            self.go(url);
        }
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
        self.view.navigate(navigation).map_err(|error| match error {
            Error::OutsidePage => {
                HostError::NavigationRefused("This page does not leave its own address.")
            }
            _ => HostError::NavigationRefused("Only http and https addresses open."),
        })
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
    /// The color of a page's face until it has drawn, as 0xRRGGBB.
    background: u32,
    /// The web panes the engine holds, as the last sync saw them.
    live: Mutex<HashSet<String>>,
    pages: Mutex<HashMap<String, Arc<WebPage>>>,
    /// Panes whose page is being made.
    making: Mutex<HashSet<String>>,
    signals: async_channel::Sender<Signal>,
    /// Why there is no web view, when there is none.
    unavailable: Mutex<Option<Error>>,
}

impl WebRegistry {
    pub(crate) fn new(
        profile: PathBuf,
        background: u32,
        signals: async_channel::Sender<Signal>,
    ) -> Arc<Self> {
        Arc::new(Self {
            profile,
            background,
            live: Mutex::new(HashSet::new()),
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

    /// Takes every page off the screen but those of the panes the window shows now. A page that is
    /// a window of its own does not go away with the pane that was drawn over it.
    pub(crate) fn hide_except(&self, shown: &HashSet<String>) {
        if !soyeht_webview::embedded() {
            return;
        }
        let pages: Vec<Arc<WebPage>> = self
            .pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(id, _)| !shown.contains(*id))
            .map(|(_, page)| Arc::clone(page))
            .collect();
        for page in pages {
            page.hide();
        }
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
        let is_live = |id: &String| live.iter().any(|pane| &pane.pane_id == id);
        *self.live.lock().unwrap_or_else(PoisonError::into_inner) =
            live.iter().map(|pane| pane.pane_id.clone()).collect();
        self.pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|id, _| is_live(id));
        // A pane that went and came back has its page made again, not left without one.
        self.making
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(is_live);
        if self.unavailable().is_some() {
            return;
        }
        for pane in live {
            if let Some(page) = self.get(&pane.pane_id) {
                page.align(pane);
                continue;
            }
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
        let handoff = Arc::new(Handoff::default());
        let person = Arc::clone(&handoff);
        let sink: ViewSink = Arc::new(move |update| match update {
            ViewUpdate::Frame => {
                // One announcement for as many pictures as the window has not taken yet.
                if !announced.swap(true, Ordering::AcqRel) {
                    sink(update);
                }
            }
            ViewUpdate::Person => {
                // The person works in the page's own window: the pane is theirs until handed back.
                person.note_person();
                sink(update);
            }
            other => sink(other),
        });
        let incognito = pane.incognito == Some(true);
        let opened = Backend::get().and_then(|backend| {
            backend.open(
                &ViewOptions {
                    profile: (!incognito).then(|| self.profile.clone()),
                    scratch: Some(self.profile.join("private")),
                    background: self.background,
                    width: FIRST_SIZE.0,
                    height: FIRST_SIZE.1,
                },
                sink,
            )
        });
        match opened {
            // The pane went while its page was being made: nobody is left to show it.
            Ok(_) if !self.is_live(&id) => self.forget(&id),
            Ok(view) => {
                let page = Arc::new(WebPage {
                    view,
                    incognito,
                    base_generation: u64::from(pane.generation),
                    handoff,
                    frame_ready,
                    reported: Mutex::new((String::new(), String::new())),
                    placed: Mutex::new(None),
                    sent: Mutex::new(None),
                });
                // The limit is set before the page goes anywhere, so that its first address is
                // already held to its own origin.
                page.view.limit_to_page(limit_of(pane));
                if let Some(url) = &pane.url {
                    page.go(url);
                }
                self.pages
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(id.clone(), page);
                self.tell(Signal::Created(id));
            }
            Err(error) => {
                soyeht_platform::log::event(
                    "desktop.web_view_unavailable",
                    &[("error", error.to_string().as_str())],
                );
                // A system with no web view says so for good; a view that was only slow or
                // closed is tried again at the next sync.
                if matches!(error, Error::Unsupported | Error::Unavailable(_)) {
                    *self
                        .unavailable
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some(error);
                } else {
                    self.forget(&id);
                }
                self.tell(Signal::Created(id));
            }
        }
    }

    fn is_live(&self, pane_id: &str) -> bool {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(pane_id)
    }

    /// Lets the next sync make the page of `pane_id` again.
    fn forget(&self, pane_id: &str) {
        self.making
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(pane_id);
    }

    /// What a page of `pane_id` reports, as a signal for the window.
    fn sink(&self, pane_id: &str) -> impl Fn(ViewUpdate) + Send + Sync + 'static {
        let (signals, pane_id) = (self.signals.clone(), pane_id.to_owned());
        move |update| {
            let signal = match update {
                ViewUpdate::Frame => Signal::Frame(pane_id.clone()),
                ViewUpdate::State | ViewUpdate::Navigating { .. } | ViewUpdate::Person => {
                    Signal::State(pane_id.clone())
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// The cases of `contracts/vectors/project-page-origin.json`, which every client passes.
    const VECTOR: &str = include_str!("../../../../../contracts/vectors/project-page-origin.json");

    fn pane(url: Option<&str>, project_path: Option<&str>) -> Pane {
        let mut pane = json!({
            "paneId": "p1", "workspaceId": "w", "name": "p1", "cwd": "/work", "mode": "web",
            "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
            "cols": 80, "rows": 24, "record": false
        });
        pane["url"] = json!(url);
        pane["projectPath"] = json!(project_path);
        serde_json::from_value(pane).unwrap()
    }

    /// What a page the engine serves may do, case by case: the top level goes where the rule says,
    /// and a frame also where the page made the address itself.
    #[test]
    fn a_limited_view_follows_the_shared_vector_for_the_page_and_its_frames() {
        let vector: Value = serde_json::from_str(VECTOR).unwrap();
        let cases = vector["cases"].as_array().unwrap();
        assert!(cases.len() > 20);
        for case in cases {
            let (page, target) = (
                case["page"].as_str().unwrap(),
                case["target"].as_str().unwrap(),
            );
            let same = case["same"].as_bool().unwrap();
            let limit = PageLimit::new(page, same_origin);
            assert_eq!(limit.allows(target), same, "{}", case["name"]);
            // A frame also holds what the page made, which the vector says is not the same origin.
            let made = ["about:blank", "about:srcdoc", "data:", "blob:"]
                .iter()
                .any(|scheme| target.to_ascii_lowercase().starts_with(scheme))
                && target.len() > "data:".len();
            let made = made || target == "about:blank";
            assert_eq!(limit.allows_frame(target), same || made, "{}", case["name"]);
        }
    }

    #[test]
    fn only_a_pane_that_shows_a_project_file_with_an_address_is_limited() {
        let url = "http://127.0.0.1:5000/0123/index.html";
        let limit = limit_of(&pane(Some(url), Some("site/index.html"))).unwrap();
        assert!(limit.allows("http://127.0.0.1:5000/0123/other.html"));
        assert!(!limit.allows("http://127.0.0.1:5001/0123/index.html"));
        assert!(limit_of(&pane(Some(url), None)).is_none());
        assert!(limit_of(&pane(None, Some("site/index.html"))).is_none());
    }

    #[test]
    fn a_page_is_sent_again_only_when_the_engine_served_it_at_another_origin() {
        let served = "http://127.0.0.1:5000/0123/index.html";
        let again = "http://127.0.0.1:6000/4567/index.html";
        // The first load has not committed yet: the page is on its way to what it was sent.
        assert!(!served_again(served, "", Some(served)));
        assert!(!served_again(served, "about:blank", Some(served)));
        // The page is where the engine says, or elsewhere in the same origin.
        assert!(!served_again(
            served,
            "http://127.0.0.1:5000/0123/other.html",
            None
        ));
        // The engine restarted and served the file at a new port and token.
        assert!(served_again(
            again,
            "http://127.0.0.1:5000/0123/other.html",
            Some(served)
        ));
        assert!(served_again(again, "", Some(served)));
        // It was sent there already: not again, whatever the page shows meanwhile.
        assert!(!served_again(
            again,
            "http://127.0.0.1:5000/0123/index.html",
            Some(again)
        ));
        // A pane that was not limited before and shows another origin is sent to its address.
        assert!(served_again(served, "https://example.com/", None));
    }
}
