//! The browser host of this client: what the engine's `web.command` asks of the page of a web
//! pane, answered the way the Mac client answers it (ADR 0055). The engine routes an agent's
//! `web_snapshot` and `web_screenshot` here; this module reads the page and the window's web
//! view does the drawing.
//!
//! The page is reached through two small traits ([`Pages`], [`Page`]) so every rule here is a
//! plain unit test with a page of its own: the access modes of the engine, the byte budgets of
//! the contract, the errors the engine accepts from a host.

mod handoff;
mod picture;
mod read;
mod read_content;
mod script;
mod snapshot;
mod stitch;

use serde_json::{Value, json};
use soyeht_protocol::{
    AttachmentRef, ErrorData, ErrorKind, RpcError, WebCommandKind, WebCommandParams,
    WebCommandResult, error_code,
};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub use handoff::Handoff;
pub use picture::Picture;

/// The key the engine adds to the arguments of a command: who is asking, and what the session may
/// do (`engine/soyeht-engine/src/web/permissions.rs`).
const ACCESS_KEY: &str = "_soyehtAccess";

/// How often a wait looks at the page again.
const WAIT_POLL: Duration = Duration::from_millis(250);
/// A page that has not changed for this long is idle.
const IDLE_FOR: Duration = Duration::from_millis(500);

/// What a host refuses with. The engine accepts these four kinds from a host and no other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    Stale(&'static str),
    HumanTookOver(&'static str),
    NavigationRefused(&'static str),
    Internal(&'static str),
}

impl HostError {
    fn rpc(&self) -> RpcError {
        let (kind, message) = match self {
            Self::Stale(message) => (ErrorKind::StaleReference, message),
            Self::HumanTookOver(message) => (ErrorKind::HumanTookOver, message),
            Self::NavigationRefused(message) => (ErrorKind::NavigationRefused, message),
            Self::Internal(message) => (ErrorKind::InternalError, message),
        };
        RpcError {
            code: error_code(&kind),
            message: (*message).to_owned(),
            data: ErrorData {
                kind,
                detail: None,
                retry_after_sec: None,
            },
        }
    }
}

/// Where a navigation goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Url(String),
    Back,
    Forward,
    Reload,
    ReloadIgnoringCache,
}

/// What the page says of itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageInfo {
    pub url: String,
    pub title: String,
    pub loading: bool,
}

/// One web pane's page, as the host uses it. Every call may block: the host calls from its own
/// threads, never from the interface.
pub trait Page: Send + Sync {
    fn incognito(&self) -> bool;
    fn navigate(&self, target: &Target) -> Result<(), HostError>;
    fn info(&self) -> PageInfo;
    /// How many navigations the page has begun, counted from the pane's stored generation.
    fn generation(&self) -> u64;
    /// Runs `source` where the page cannot see it and returns its value as text.
    fn run_script(&self, source: &str) -> Result<String, HostError>;
    /// The engine's own picture of what the pane shows.
    fn picture(&self) -> Result<Picture, HostError>;
    /// One scroll of the wheel at a point of the page, `lines` of it, positive going down: what
    /// a person's wheel does.
    fn wheel(&self, x: f64, y: f64, lines: f64);
    /// Who has the pane.
    fn handoff(&self) -> &Handoff;
    fn has_cookies(&self, url: &str) -> Result<bool, HostError>;
}

/// The pages of the window, found by the pane that shows them.
pub trait Pages: Send + Sync {
    fn page(&self, pane_id: &str) -> Option<Arc<dyn Page>>;
}

/// What a command answers: the result for the engine, and the picture that goes before it.
#[derive(Debug)]
pub struct Outcome {
    pub result: Result<WebCommandResult, RpcError>,
    pub attachment: Option<(AttachmentRef, Vec<u8>)>,
}

impl Outcome {
    fn ok(pane_id: &str, value: Value) -> Self {
        Self {
            result: Ok(WebCommandResult {
                pane_id: pane_id.to_owned(),
                value: Some(value),
                cookies_present: None,
            }),
            attachment: None,
        }
    }

    fn refused(error: &HostError) -> Self {
        Self {
            result: Err(error.rpc()),
            attachment: None,
        }
    }
}

/// Answers `web.command`.
pub struct Host {
    pages: Arc<dyn Pages>,
}

/// Who may do what in the session, from the engine's own note in the arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Session {
    /// The person asked: no permission question.
    Human,
    /// A private pane: nothing of the profile is in it.
    Incognito,
    /// An agent has not been allowed to use the profile's cookies yet: the host says whether
    /// the page's site has any before it does anything.
    Probe,
    /// The agent was allowed.
    Granted,
}

impl Host {
    pub fn new(pages: Arc<dyn Pages>) -> Self {
        Self { pages }
    }

    /// What the host answers a request it cannot read.
    pub fn refusal(message: &'static str) -> Outcome {
        Outcome::refused(&HostError::Internal(message))
    }

    pub fn execute(&self, params: &WebCommandParams) -> Outcome {
        match self.run(params) {
            Ok(outcome) => outcome,
            Err(error) => Outcome::refused(&error),
        }
    }

    fn run(&self, params: &WebCommandParams) -> Result<Outcome, HostError> {
        let page = self
            .pages
            .page(&params.pane_id)
            .ok_or(HostError::Internal("The web pane is unavailable."))?;
        let session = session(&params.args, page.incognito())?;
        // An agent drives only a pane the person has not taken.
        if session != Session::Human {
            page.handoff().agent_may_drive()?;
        }
        if session == Session::Probe {
            let host_url = probe_url(&params.args, &page);
            if let Some(url) = host_url
                && page.has_cookies(&url)?
            {
                return Ok(Outcome {
                    result: Ok(WebCommandResult {
                        pane_id: params.pane_id.clone(),
                        value: None,
                        cookies_present: Some(true),
                    }),
                    attachment: None,
                });
            }
            let mut outcome = self.perform(&page, params)?;
            if let Ok(result) = &mut outcome.result {
                result.cookies_present = Some(false);
            }
            return Ok(outcome);
        }
        self.perform(&page, params)
    }

    fn perform(
        &self,
        page: &Arc<dyn Page>,
        params: &WebCommandParams,
    ) -> Result<Outcome, HostError> {
        let pane = params.pane_id.as_str();
        match params.command {
            WebCommandKind::Navigate => {
                page.navigate(&target(&params.args)?)?;
                Ok(Outcome::ok(pane, json!({"accepted": true})))
            }
            WebCommandKind::Snapshot => snapshot::take(page.as_ref(), pane, &params.args),
            WebCommandKind::Read => {
                read::read(page.as_ref(), pane, &params.args, params.timeout_ms)
            }
            WebCommandKind::Screenshot if flag(&params.args, "fullPage") => {
                read::full_page(page.as_ref(), pane)
            }
            WebCommandKind::Screenshot => {
                let (reference, bytes) = picture::screenshot(&page.picture()?)?;
                let mut outcome = Outcome::ok(pane, json!({"image": reference}));
                outcome.attachment = Some((reference, bytes));
                Ok(outcome)
            }
            WebCommandKind::Scroll => {
                script::run(page.as_ref(), &script::scroll(&params.args))?;
                Ok(Outcome::ok(pane, json!({"completed": true})))
            }
            WebCommandKind::Wait => {
                let wait = script::Wait::from_args(&params.args);
                let limit = Duration::from_millis(u64::from(params.timeout_ms));
                let matched = wait_for(page.as_ref(), &wait, limit)?;
                Ok(Outcome::ok(pane, json!({"matched": matched})))
            }
            _ => Err(HostError::Internal(
                "The web command is not implemented in this client yet.",
            )),
        }
    }
}

fn session(args: &Value, incognito: bool) -> Result<Session, HostError> {
    let mode = args
        .get(ACCESS_KEY)
        .and_then(|access| access.get("session"))
        .and_then(Value::as_str);
    match mode {
        Some("human") => Ok(Session::Human),
        Some("incognito") if incognito => Ok(Session::Incognito),
        Some("probe") if !incognito => Ok(Session::Probe),
        Some("granted") if !incognito => Ok(Session::Granted),
        _ => Err(HostError::Internal("The web command access is invalid.")),
    }
}

/// The address whose cookies a probe asks about: the one a navigation goes to, else the page's.
fn probe_url(args: &Value, page: &Arc<dyn Page>) -> Option<String> {
    args.get("url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| Some(page.info().url).filter(|url| !url.is_empty()))
}

fn target(args: &Value) -> Result<Target, HostError> {
    if let Some(url) = args.get("url").and_then(Value::as_str) {
        return Ok(Target::Url(url.to_owned()));
    }
    match args.get("action").and_then(Value::as_str) {
        Some("back") => Ok(Target::Back),
        Some("forward") => Ok(Target::Forward),
        Some("reload") => Ok(Target::Reload),
        Some("reload_ignoring_cache") => Ok(Target::ReloadIgnoringCache),
        _ => Err(HostError::NavigationRefused(
            "The navigation target is invalid.",
        )),
    }
}

fn flag(args: &Value, name: &str) -> bool {
    args.get(name).and_then(Value::as_bool) == Some(true)
}

/// Looks at the page until its conditions hold or `limit` passes.
fn wait_for(page: &dyn Page, wait: &script::Wait, limit: Duration) -> Result<bool, HostError> {
    let started = std::time::Instant::now();
    let mut quiet_since = started;
    let mut last = page.info();
    loop {
        let info = page.info();
        if info != last {
            quiet_since = std::time::Instant::now();
            last = info;
        }
        let idle = !last.loading && quiet_since.elapsed() >= IDLE_FOR;
        if wait.holds(page, idle)? {
            return Ok(true);
        }
        if started.elapsed() >= limit {
            return Ok(false);
        }
        std::thread::sleep(WAIT_POLL);
    }
}

/// An attachment's name for the engine: a new identity, the PNG's size and the fixed name the
/// engine requires.
fn attachment(bytes: &[u8]) -> AttachmentRef {
    AttachmentRef {
        attachment_id: Uuid::new_v4().to_string(),
        mime_type: "image/png".to_owned(),
        name: "screenshot.png".to_owned(),
        bytes: bytes.len().to_string(),
    }
}

/// The limit of a web response's text, in bytes.
fn clip(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests;
