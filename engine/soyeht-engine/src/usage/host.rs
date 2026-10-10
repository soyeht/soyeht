//! The engine's [`Host`]: the real file system, keychain, HTTPS, browser cookies and pseudo
//! terminal behind the usage sources.

use super::{process, terminal};
use crate::agents::{environment::launch_path, registry::executable};
use soyeht_usage::host::{
    CookieRead, FileRead, Host, HttpRequest, HttpResult, Platform, RpcRequest, RpcRun, SecretRead,
    TerminalDriver, TerminalRequest, TerminalRun,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// The real host. Debug builds started with `SOYEHT_TEST_USAGE_READER_DIR` instead read a
/// fixture directory for the end-to-end suite: `home/` replaces the home directory, and the
/// `http-source`, `keychain-source` and `second-source` scripts replace curl, `security` and
/// `codex`; browsers and terminals are never touched.
#[derive(Default)]
pub struct SystemHost {
    fixtures: Option<PathBuf>,
}

impl SystemHost {
    pub fn from_environment() -> Self {
        #[cfg(debug_assertions)]
        if let Some(directory) = std::env::var_os("SOYEHT_TEST_USAGE_READER_DIR") {
            return Self {
                fixtures: Some(PathBuf::from(directory)),
            };
        }
        Self::default()
    }

    /// Whether the background schedule runs: the end-to-end suite drives refreshes itself.
    pub fn schedules(&self) -> bool {
        self.fixtures.is_none()
    }

    fn fixture(&self, name: &str) -> Option<PathBuf> {
        self.fixtures.as_ref().map(|directory| directory.join(name))
    }
}

impl Host for SystemHost {
    fn platform(&self) -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    fn env(&self, name: &str) -> Option<String> {
        if self.fixtures.is_some() {
            return None;
        }
        std::env::var(name).ok()
    }

    fn home(&self) -> Option<PathBuf> {
        if let Some(home) = self.fixture("home") {
            return Some(home);
        }
        std::env::home_dir().filter(|home| home.is_absolute())
    }

    /// The canonical temporary directory: Claude Code names a project after its real path.
    fn temp_dir(&self) -> PathBuf {
        let temp = std::env::temp_dir();
        soyeht_platform::fs::canonicalize(&temp).unwrap_or(temp)
    }

    fn read_file(&self, path: &Path, max_bytes: u64) -> FileRead {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return FileRead::Missing;
            }
            Err(_) => return FileRead::Unreadable,
        };
        if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
            return FileRead::Unreadable;
        }
        let mut bytes = Vec::new();
        match file
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
        {
            Ok(count) if count as u64 <= max_bytes => FileRead::Bytes(bytes),
            _ => FileRead::Unreadable,
        }
    }

    fn keychain_password(&self, service: &str, account: Option<&str>) -> SecretRead {
        if self.platform() != Platform::MacOs {
            return SecretRead::Unsupported;
        }
        let program = self
            .fixture("keychain-source")
            .unwrap_or_else(|| PathBuf::from(process::SECURITY));
        process::keychain_password(&program, service, account)
    }

    fn http(&self, request: &HttpRequest) -> HttpResult {
        let program = self
            .fixture("http-source")
            .unwrap_or_else(|| PathBuf::from(process::CURL));
        process::http(&program, request)
    }

    fn browser_cookies(&self, domain: &str) -> CookieRead {
        #[cfg(target_os = "macos")]
        if self.fixtures.is_none()
            && let Some(home) = self.home()
        {
            return super::browser::cookies(&home, domain);
        }
        let _ = domain;
        CookieRead::default()
    }

    fn state_db_value(&self, path: &Path, key: &str) -> SecretRead {
        soyeht_usage::foreign_db::item_table_value(path, key)
    }

    fn json_rpc(&self, request: &RpcRequest) -> RpcRun {
        let program = match self.fixture("second-source") {
            Some(fixture) => Some(fixture),
            None => executable(&request.program),
        };
        let Some(program) = program else {
            return RpcRun::ToolNotFound;
        };
        process::json_rpc(&program, &launch_path(), request)
    }

    fn run_terminal(
        &self,
        request: &TerminalRequest,
        driver: &mut dyn TerminalDriver,
    ) -> TerminalRun {
        let program = executable(&request.program).filter(|_| self.fixtures.is_none());
        let Some(program) = program else {
            return TerminalRun::ToolNotFound;
        };
        // The scratch directory must be Soyeht's own, inside the temporary directory.
        if !request.workdir.starts_with(self.temp_dir()) {
            return TerminalRun::Incomplete;
        }
        terminal::run(&program, &launch_path(), request, driver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn files_over_their_bound_are_unreadable() {
        let path = std::env::temp_dir().join(format!("sn-bound-{}", Uuid::new_v4()));
        fs::write(&path, b"0123456789").unwrap();
        assert_eq!(
            SystemHost::default().read_file(&path, 10),
            FileRead::Bytes(b"0123456789".to_vec())
        );
        assert_eq!(
            SystemHost::default().read_file(&path, 9),
            FileRead::Unreadable
        );
        assert_eq!(
            SystemHost::default().read_file(&path.with_extension("absent"), 9),
            FileRead::Missing
        );
        fs::remove_file(path).unwrap();
    }
}
