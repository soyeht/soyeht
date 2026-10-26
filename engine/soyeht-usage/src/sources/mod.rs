//! One usage source per provider and method, all behind [`Source`].
//!
//! The chains, their order and the fallback rules follow CodexBar
//! (github.com/steipete/CodexBar, MIT, © Peter Steinberger), reimplemented for Soyeht's engine.

pub(crate) mod claude;
pub(crate) mod codex;
pub(crate) mod cursor;
mod jwt;

use crate::{
    Window,
    host::{CookieJar, CookieRead, FileRead, Host, HttpResult, Platform, SecretRead},
};
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde_json::Value;
use soyeht_protocol::{UsageProvider, UsageSource};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

/// What the caller of a refresh is: CodexBar lets a user's own click bypass the cooldowns that
/// protect background refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Automatic,
    Manual,
}

/// What one refresh of one provider shares between its sources.
#[derive(Clone)]
pub(crate) struct ReadContext {
    pub now_sec: u64,
    keychain: Arc<Mutex<Vec<(String, SecretRead)>>>,
}

impl ReadContext {
    pub(crate) fn new(now_sec: u64) -> Self {
        Self {
            now_sec,
            keychain: Arc::default(),
        }
    }

    /// A keychain item, asked for at most once per refresh: every ask can show the person a
    /// consent prompt, and two sources of one refresh may need the same item.
    pub(crate) fn keychain_password(&self, host: &dyn Host, service: &str) -> SecretRead {
        let mut asked = self.keychain.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, read)) = asked.iter().find(|(name, _)| name == service) {
            return read.clone();
        }
        let read = host.keychain_password(service, None);
        asked.push((service.to_owned(), read.clone()));
        read
    }
}

/// A normalized reading before the planner stamps provider and source on it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedReading {
    pub primary: Window,
    pub secondary: Option<Window>,
}

/// Why one source produced no reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Nothing to read from: no credential, cookie or login on this machine. The source was
    /// not attempted.
    NotConfigured,
    ToolNotFound,
    /// The provider rejected the credential (HTTP 401, an expired token, a CLI login prompt).
    LoginNeeded,
    /// The user or the provider refused access (a declined keychain prompt, HTTP 403).
    NoAccess,
    RateLimited {
        retry_after_sec: Option<u32>,
    },
    Network,
    /// A response that does not have the expected shape.
    InvalidResponse,
    /// A well-formed answer that carries no reading yet: every window is absent or null.
    NoData,
    /// A tool that ran but could not produce usage (failed app server, incomplete screen).
    Failed,
}

impl Failure {
    pub(crate) fn attempted(&self) -> bool {
        !matches!(self, Self::NotConfigured)
    }
}

pub(crate) type Attempt = Result<ParsedReading, Failure>;

pub(crate) trait Source: Send + Sync {
    fn id(&self) -> UsageSource;
    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt;
    /// Whether the chain continues to the next source after this failure.
    fn falls_back(&self, _failure: &Failure) -> bool {
        true
    }
    /// For a source that starts a provider process: how long an automatic refresh reuses its
    /// last reading instead of starting the process again.
    fn automatic_reuse_sec(&self) -> Option<u64> {
        None
    }
    /// How long automatic refreshes skip this source after `failure`. A manual refresh always
    /// tries again, as a click does in CodexBar.
    fn automatic_cooldown_sec(&self, failure: &Failure) -> Option<u64> {
        default_cooldown_sec(failure)
    }
}

/// CodexBar's gates: a rate limit blocks until `Retry-After`, or five minutes without one
/// (`ClaudeOAuthUsageRateLimitGate`, `ClaudeCLIRateLimitGate`); a declined keychain or cookie
/// prompt is not asked again for six hours (`BrowserCookieAccessGate`,
/// `ClaudeOAuthKeychainAccessGate`).
pub(crate) fn default_cooldown_sec(failure: &Failure) -> Option<u64> {
    match failure {
        Failure::RateLimited { retry_after_sec } => Some(
            retry_after_sec
                .map(u64::from)
                .filter(|seconds| *seconds > 0)
                .unwrap_or(RATE_LIMIT_COOLDOWN_SEC),
        ),
        Failure::NoAccess => Some(DENIED_COOLDOWN_SEC),
        _ => None,
    }
}

const RATE_LIMIT_COOLDOWN_SEC: u64 = 5 * 60;
const DENIED_COOLDOWN_SEC: u64 = 6 * 60 * 60;

pub(crate) fn chain(provider: &UsageProvider) -> Vec<Box<dyn Source>> {
    match provider {
        UsageProvider::Claude => claude::chain(),
        UsageProvider::Codex => codex::chain(),
        UsageProvider::Cursor => cursor::chain(),
    }
}

pub(crate) const TOKEN_FILE_MAX_BYTES: u64 = 256 * 1024;

pub(crate) fn json(bytes: &[u8]) -> Result<Value, Failure> {
    serde_json::from_slice(bytes).map_err(|_| Failure::InvalidResponse)
}

/// The body of a successful response, or the failure its status means.
pub(crate) fn http_body(result: HttpResult) -> Result<Vec<u8>, Failure> {
    match result {
        HttpResult::TransportFailed => Err(Failure::Network),
        HttpResult::Response {
            status,
            retry_after_sec,
            body,
        } => match status {
            200..=299 => Ok(body),
            401 => Err(Failure::LoginNeeded),
            403 => Err(Failure::NoAccess),
            429 => Err(Failure::RateLimited { retry_after_sec }),
            408 | 500..=599 => Err(Failure::Network),
            _ => Err(Failure::Failed),
        },
    }
}

pub(crate) fn read_json_file(host: &dyn Host, path: &Path) -> Result<Value, Failure> {
    match host.read_file(path, TOKEN_FILE_MAX_BYTES) {
        FileRead::Bytes(bytes) => json(&bytes),
        FileRead::Missing => Err(Failure::NotConfigured),
        FileRead::Unreadable => Err(Failure::NoAccess),
    }
}

/// `$NAME` when set and non-empty, as a path; relative values resolve against the home.
pub(crate) fn env_dir(host: &dyn Host, name: &str) -> Option<PathBuf> {
    let value = host.env(name).filter(|value| !value.trim().is_empty())?;
    let path = PathBuf::from(value.trim());
    if path.is_absolute() {
        Some(path)
    } else {
        host.home().map(|home| home.join(path))
    }
}

/// A used percentage (0–100) as the wire fraction, clamped the way CodexBar clamps it.
pub(crate) fn percent_window(
    percent: f64,
    resets_at: Option<String>,
    label: &str,
) -> Result<Window, Failure> {
    if !percent.is_finite() {
        return Err(Failure::InvalidResponse);
    }
    let used = (percent / 100.0).clamp(0.0, 1.0);
    Window::checked(used, resets_at.unwrap_or_default(), label).ok_or(Failure::InvalidResponse)
}

/// The first browser session that the provider accepts, trying each browser profile's
/// cookies for `domains` in order (macOS only, as in CodexBar). A rejected session belongs to a
/// signed-out profile, so the next one is tried; any other failure is the answer.
pub(crate) fn first_browser_session<T>(
    host: &dyn Host,
    domains: &[&str],
    session: impl Fn(&CookieJar) -> Option<T>,
    read: impl Fn(T) -> Attempt,
) -> Attempt {
    if host.platform() != Platform::MacOs {
        return Err(Failure::NotConfigured);
    }
    let mut found = CookieRead::default();
    for domain in domains {
        let read = host.browser_cookies(domain);
        found.denied |= read.denied;
        found.jars.extend(read.jars);
    }
    let mut last = found.nothing_found();
    for candidate in found.jars.iter().filter_map(session) {
        match read(candidate) {
            Ok(reading) => return Ok(reading),
            Err(Failure::LoginNeeded | Failure::NoAccess) => last = Failure::LoginNeeded,
            Err(failure) => return Err(failure),
        }
    }
    Err(last)
}

pub(crate) fn epoch_text(seconds: i64) -> Option<String> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// A provider's reset time given as Unix seconds, either a JSON number or a string of digits
/// (the Codex app server sends a string), as the wire's ISO 8601 UTC text (contract 10).
/// Anything else, including a negative or fractional count, is no reset time.
pub(crate) fn epoch_value(value: &Value) -> Option<String> {
    /// Far beyond any reset time, and below where the count stops being exact.
    const LAST_SECOND: f64 = 1e12;
    let seconds = number(value).filter(|seconds| (0.0..=LAST_SECOND).contains(seconds))?;
    if seconds.fract() != 0.0 {
        return None;
    }
    epoch_text(seconds as i64)
}

/// An ISO 8601 timestamp normalized to UTC, with or without fractional seconds.
pub(crate) fn iso_text(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value.trim()).ok().map(|time| {
        time.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    })
}

pub(crate) fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}
