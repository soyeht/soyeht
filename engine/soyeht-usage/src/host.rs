//! The input/output a usage source needs, behind one port.
//!
//! Sources in this crate decide *what* to read and how to interpret it; the engine implements
//! [`Host`] with the real file system, keychain, HTTP client, browser cookie stores and pseudo
//! terminal. Tests implement it with recorded fixtures, so no test touches the network or a
//! real credential.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

/// A credential or cookie value. It never prints, so it cannot reach a log by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

/// Upper bound for one token or cookie value.
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Printable ASCII without example or quotes, within the size bound: safe to place in a
    /// header without escaping.
    pub fn is_usable(&self) -> bool {
        let value = self.0.as_bytes();
        !value.is_empty()
            && value.len() <= MAX_SECRET_BYTES
            && value
                .iter()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

/// The operating system the engine runs on. Credential stores differ: macOS keeps Claude
/// Code's login in the keychain and browsers' cookie keys in it, Linux keeps the login in a
/// file and has no supported browser store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRead {
    Bytes(Vec<u8>),
    Missing,
    /// Larger than the caller's bound, unreadable, or not a regular file.
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRead {
    Found(Secret),
    Missing,
    /// The store refused, for example the user declined the macOS keychain prompt.
    Denied,
    /// This platform has no such store.
    Unsupported,
}

/// An HTTPS GET.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    /// Plain headers.
    pub headers: Vec<(String, String)>,
    /// Headers that carry a credential (Authorization, Cookie). A host keeps them off any
    /// command line, log or error.
    pub secret_headers: Vec<(String, Secret)>,
    pub timeout: Duration,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
            secret_headers: Vec::new(),
            timeout,
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn secret_header(mut self, name: &str, value: Secret) -> Self {
        self.secret_headers.push((name.to_owned(), value));
        self
    }

    /// `Authorization: Bearer <token>`, kept with the other credential headers.
    pub fn bearer(self, token: &Secret) -> Self {
        self.secret_header(
            "Authorization",
            Secret::new(format!("Bearer {}", token.expose())),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpResult {
    Response {
        status: u16,
        retry_after_sec: Option<u32>,
        body: Vec<u8>,
    },
    /// DNS, TLS, connection, timeout or an oversized body.
    TransportFailed,
}

/// A cookie copied from one of the user's browsers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: Secret,
}

/// The cookies one browser profile holds for a domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieJar {
    pub cookies: Vec<Cookie>,
}

impl CookieJar {
    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.cookies
            .iter()
            .find(|cookie| cookie.name == name)
            .map(|cookie| &cookie.value)
    }

    /// The `Cookie:` header value for every cookie in the jar.
    pub fn header(&self) -> Secret {
        Secret::new(
            self.cookies
                .iter()
                .map(|cookie| format!("{}={}", cookie.name, cookie.value.expose()))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

/// The cookie jars found for a domain. `denied` means a browser's cookie key was refused (the
/// user declined the keychain prompt), so that browser's cookies could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CookieRead {
    pub jars: Vec<CookieJar>,
    pub denied: bool,
}

impl CookieRead {
    /// The failure to report when no jar holds a session.
    pub(crate) fn nothing_found(&self) -> crate::Failure {
        if self.denied {
            crate::Failure::NoAccess
        } else {
            crate::Failure::NotConfigured
        }
    }
}

/// What a terminal driver wants after looking at the current screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalStep {
    Wait,
    Send(Vec<u8>),
    /// The screen holds what the driver needs; the host stops the process.
    Done,
}

/// Drives an interactive command line through a pseudo terminal, one screen at a time.
pub trait TerminalDriver {
    /// `screen` is the visible text of the terminal; `elapsed` counts from the start.
    fn step(&mut self, screen: &str, elapsed: Duration) -> TerminalStep;
}

/// One run of a command line in a pseudo terminal.
///
/// The host creates `workdir` fresh, writes `files` into it, runs the command there, then
/// removes the directory and the session transcript named by `transcript`, so a run leaves
/// nothing behind.
pub struct TerminalRequest {
    pub program: String,
    pub args: Vec<String>,
    /// Extra environment on top of the host's minimal one (HOME, PATH, locale, TERM).
    pub env: Vec<(String, String)>,
    pub workdir: PathBuf,
    /// Relative path inside `workdir` and its contents.
    pub files: Vec<(String, Vec<u8>)>,
    /// The transcript file the command writes for this session. The host removes it after
    /// the run, and its directory too when that is left empty.
    pub transcript: Option<PathBuf>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalRun {
    /// The driver returned [`TerminalStep::Done`]; the last screen it saw.
    Finished(String),
    ToolNotFound,
    /// The deadline passed, or the process exited, before the driver finished.
    Incomplete,
}

/// One JSON-RPC exchange over a child process's standard input and output, one JSON
/// document per line. Calls run in order; notifications (no id) get no answer.
pub struct RpcRequest {
    pub program: String,
    pub args: Vec<String>,
    pub calls: Vec<RpcCall>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub method: String,
    pub params: serde_json::Value,
    pub id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RpcRun {
    /// The `result` of every call that has an id, in call order.
    Results(Vec<serde_json::Value>),
    ToolNotFound,
    Failed,
}

/// Every primitive a usage source may use. Implementations never log credentials or cookies,
/// never write to another application's files, and bound every read and every wait.
pub trait Host: Send + Sync {
    fn platform(&self) -> Platform;
    fn env(&self, name: &str) -> Option<String>;
    fn home(&self) -> Option<PathBuf>;
    /// The scratch directory under which terminal runs get their working directory.
    fn temp_dir(&self) -> PathBuf;
    fn read_file(&self, path: &Path, max_bytes: u64) -> FileRead;
    /// A generic password from the macOS login keychain; `Unsupported` elsewhere.
    fn keychain_password(&self, service: &str, account: Option<&str>) -> SecretRead;
    fn http(&self, request: &HttpRequest) -> HttpResult;
    /// Cookies for `domain` and its subdomains, one jar per browser profile that has any, in
    /// the host's browser order. Empty where the platform has no supported browser store.
    fn browser_cookies(&self, domain: &str) -> CookieRead;
    /// A value from the `ItemTable` of an Electron state database, opened read-only.
    fn state_db_value(&self, path: &Path, key: &str) -> SecretRead;
    fn json_rpc(&self, request: &RpcRequest) -> RpcRun;
    fn run_terminal(
        &self,
        request: &TerminalRequest,
        driver: &mut dyn TerminalDriver,
    ) -> TerminalRun;
}
