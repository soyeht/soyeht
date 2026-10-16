//! A host for the end-to-end suite: a fixture directory in place of the machine.
//!
//! `home/` stands in for the home directory, and the `http-source`, `keychain-source` and
//! `second-source` scripts for the HTTP client, the keychain tool and the JSON-RPC provider
//! program. Browsers, terminals and the environment are never touched. Compiled only with the `test-hooks` feature (ADR 0091).

use super::{SystemHost, process};
use crate::agents::environment::launch_path;
use soyeht_usage::host::{
    CookieRead, FileRead, Host, HttpRequest, HttpResult, Platform, RpcRequest, RpcRun, SecretRead,
    TerminalDriver, TerminalRequest, TerminalRun,
};
use std::path::{Path, PathBuf};

/// What it does not replace it asks of the real host.
pub struct FixtureHost {
    directory: PathBuf,
    system: SystemHost,
}

impl FixtureHost {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            system: SystemHost,
        }
    }

    fn fixture(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
}

impl Host for FixtureHost {
    fn platform(&self) -> Platform {
        self.system.platform()
    }

    fn env(&self, _name: &str) -> Option<String> {
        None
    }

    fn home(&self) -> Option<PathBuf> {
        Some(self.fixture("home"))
    }

    fn temp_dir(&self) -> PathBuf {
        self.system.temp_dir()
    }

    fn read_file(&self, path: &Path, max_bytes: u64) -> FileRead {
        self.system.read_file(path, max_bytes)
    }

    fn keychain_password(&self, service: &str, account: Option<&str>) -> SecretRead {
        match self.platform() {
            Platform::MacOs => {
                process::keychain_password(&self.fixture("keychain-source"), service, account)
            }
            _ => SecretRead::Unsupported,
        }
    }

    fn http(&self, request: &HttpRequest) -> HttpResult {
        process::http(&self.fixture("http-source"), request)
    }

    fn browser_cookies(&self, _domain: &str) -> CookieRead {
        CookieRead::default()
    }

    fn state_db_value(&self, path: &Path, key: &str) -> SecretRead {
        self.system.state_db_value(path, key)
    }

    fn json_rpc(&self, request: &RpcRequest) -> RpcRun {
        process::json_rpc(&self.fixture("second-source"), &launch_path(), request)
    }

    fn run_terminal(
        &self,
        _request: &TerminalRequest,
        _driver: &mut dyn TerminalDriver,
    ) -> TerminalRun {
        TerminalRun::ToolNotFound
    }
}
