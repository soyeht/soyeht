//! A scripted [`Host`]: recorded responses in, every call logged out. No test touches the
//! network, the keychain, a browser or a real provider process.
#![allow(dead_code)]

use soyeht_usage::host::{
    CookieJar, CookieRead, FileRead, Host, HttpRequest, HttpResult, Platform, RpcRequest, RpcRun,
    SecretRead, TerminalDriver, TerminalRequest, TerminalRun, TerminalStep,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

pub const HOME: &str = "/home/tester";
pub const TEMP: &str = "/tmp/scratch";

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Http {
        url: String,
        headers: Vec<(String, String)>,
        /// Only the names of credential headers are logged.
        secret_names: Vec<String>,
        secret_values: Vec<String>,
    },
    Keychain(String),
    Cookies(String),
    StateDb(PathBuf),
    Rpc {
        program: String,
        args: Vec<String>,
        methods: Vec<String>,
    },
    Terminal {
        program: String,
        args: Vec<String>,
        workdir: PathBuf,
        transcript: Option<PathBuf>,
        sent: Vec<Vec<u8>>,
    },
}

pub struct FakeHost {
    pub platform: Platform,
    pub env: Vec<(String, String)>,
    pub files: Vec<(PathBuf, FileRead)>,
    pub keychain: SecretRead,
    pub cookies: Vec<(String, CookieJar)>,
    pub cookies_denied: bool,
    pub state_db: SecretRead,
    pub http: Mutex<VecDeque<(String, HttpResult)>>,
    pub rpc: Mutex<VecDeque<RpcRun>>,
    /// Screens shown to the terminal driver, one per half second; `None` means no `claude`.
    pub screens: Option<Vec<String>>,
    pub log: Mutex<Vec<Call>>,
}

impl FakeHost {
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            env: Vec::new(),
            files: Vec::new(),
            keychain: SecretRead::Missing,
            cookies: Vec::new(),
            cookies_denied: false,
            state_db: SecretRead::Missing,
            http: Mutex::new(VecDeque::new()),
            rpc: Mutex::new(VecDeque::new()),
            screens: None,
            log: Mutex::new(Vec::new()),
        }
    }

    pub fn file(mut self, relative: &str, bytes: &[u8]) -> Self {
        self.files.push((
            Path::new(HOME).join(relative),
            FileRead::Bytes(bytes.to_vec()),
        ));
        self
    }

    pub fn respond(self, url: &str, status: u16, body: &[u8]) -> Self {
        self.respond_with(
            url,
            HttpResult::Response {
                status,
                retry_after_sec: None,
                body: body.to_vec(),
            },
        )
    }

    pub fn respond_with(self, url: &str, result: HttpResult) -> Self {
        self.http
            .lock()
            .unwrap()
            .push_back((url.to_owned(), result));
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.log.lock().unwrap().clone()
    }

    pub fn http_urls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Http { url, .. } => Some(url),
                _ => None,
            })
            .collect()
    }

    pub fn terminal_runs(&self) -> usize {
        self.calls()
            .iter()
            .filter(|call| matches!(call, Call::Terminal { .. }))
            .count()
    }

    pub fn rpc_runs(&self) -> usize {
        self.calls()
            .iter()
            .filter(|call| matches!(call, Call::Rpc { .. }))
            .count()
    }

    fn record(&self, call: Call) {
        self.log.lock().unwrap().push(call);
    }
}

impl Host for FakeHost {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn env(&self, name: &str) -> Option<String> {
        self.env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }

    fn home(&self) -> Option<PathBuf> {
        Some(PathBuf::from(HOME))
    }

    fn temp_dir(&self) -> PathBuf {
        PathBuf::from(TEMP)
    }

    fn read_file(&self, path: &Path, _max_bytes: u64) -> FileRead {
        self.files
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map_or(FileRead::Missing, |(_, read)| read.clone())
    }

    fn keychain_password(&self, service: &str, _account: Option<&str>) -> SecretRead {
        self.record(Call::Keychain(service.to_owned()));
        self.keychain.clone()
    }

    fn http(&self, request: &HttpRequest) -> HttpResult {
        self.record(Call::Http {
            url: request.url.clone(),
            headers: request.headers.clone(),
            secret_names: request
                .secret_headers
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            secret_values: request
                .secret_headers
                .iter()
                .map(|(_, value)| value.expose().to_owned())
                .collect(),
        });
        let mut queue = self.http.lock().unwrap();
        let index = queue.iter().position(|(url, _)| *url == request.url);
        index
            .and_then(|index| queue.remove(index))
            .map_or(HttpResult::TransportFailed, |(_, result)| result)
    }

    fn browser_cookies(&self, domain: &str) -> CookieRead {
        self.record(Call::Cookies(domain.to_owned()));
        CookieRead {
            jars: self
                .cookies
                .iter()
                .filter(|(candidate, _)| candidate == domain)
                .map(|(_, jar)| jar.clone())
                .collect(),
            denied: self.cookies_denied,
        }
    }

    fn state_db_value(&self, path: &Path, _key: &str) -> SecretRead {
        self.record(Call::StateDb(path.to_owned()));
        self.state_db.clone()
    }

    fn json_rpc(&self, request: &RpcRequest) -> RpcRun {
        self.record(Call::Rpc {
            program: request.program.clone(),
            args: request.args.clone(),
            methods: request
                .calls
                .iter()
                .map(|call| call.method.clone())
                .collect(),
        });
        self.rpc
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(RpcRun::ToolNotFound)
    }

    fn run_terminal(
        &self,
        request: &TerminalRequest,
        driver: &mut dyn TerminalDriver,
    ) -> TerminalRun {
        let mut sent = Vec::new();
        let outcome = match &self.screens {
            None => TerminalRun::ToolNotFound,
            Some(screens) => {
                let mut outcome = TerminalRun::Incomplete;
                let mut elapsed = Duration::ZERO;
                let mut index = 0;
                while elapsed <= request.timeout {
                    let screen = &screens[index.min(screens.len() - 1)];
                    match driver.step(screen, elapsed) {
                        TerminalStep::Done => {
                            outcome = TerminalRun::Finished(screen.clone());
                            break;
                        }
                        TerminalStep::Send(bytes) => sent.push(bytes),
                        TerminalStep::Wait => {}
                    }
                    index += 1;
                    elapsed += Duration::from_millis(500);
                }
                outcome
            }
        };
        self.record(Call::Terminal {
            program: request.program.clone(),
            args: request.args.clone(),
            workdir: request.workdir.clone(),
            transcript: request.transcript.clone(),
            sent,
        });
        outcome
    }
}
