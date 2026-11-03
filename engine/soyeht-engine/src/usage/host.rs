//! The engine's [`Host`]: the real file system, keychain, HTTPS, browser cookies and pseudo
//! terminal behind the usage sources.

use super::{process, terminal};
use crate::{
    agents::{environment::launch_path, registry::executable},
    streak::{Streak, first_failure, recovered},
};
use soyeht_platform::log;
use soyeht_usage::host::{
    CookieRead, FileRead, Host, HttpRequest, HttpResult, Platform, RpcRequest, RpcRun, SecretRead,
    TerminalDriver, TerminalRequest, TerminalRun,
};
use soyeht_usage::host::{LocalUsageRead, StoredCredential};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// A state database that cannot be read, counted across refreshes: it is read on each one.
static STATE_DB: Mutex<Streak> = Mutex::new(Streak::once());

/// OpenCode's database when it cannot be read, counted across refreshes: it is read on each one.
static OPENCODE_DB: Mutex<Streak> = Mutex::new(Streak::once());

/// The real host: the machine's files, keychain, HTTPS, browser cookies and pseudo terminal.
pub struct SystemHost;

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
        // ignored: an unset or non-Unicode variable counts as not set
        std::env::var(name).ok()
    }

    fn home(&self) -> Option<PathBuf> {
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
            // ignored: `Unreadable` reaches the reading as an unavailable source.
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
        process::keychain_password(Path::new(process::SECURITY), service, account)
    }

    fn http(&self, request: &HttpRequest) -> HttpResult {
        process::http(Path::new(process::CURL), request)
    }

    #[cfg_attr(
        not(target_os = "macos"),
        allow(unused_variables, reason = "only macOS reads browser cookies")
    )]
    fn browser_cookies(&self, domain: &str) -> CookieRead {
        #[cfg(target_os = "macos")]
        if let Some(home) = self.home() {
            return super::browser::cookies(&home, domain);
        }
        CookieRead::default()
    }

    fn state_db_value(&self, path: &Path, key: &str) -> SecretRead {
        match soyeht_usage::foreign_db::item_table_value(path, key) {
            Ok(read) => {
                recovered(&STATE_DB);
                read
            }
            Err(unreadable) => {
                if first_failure(&STATE_DB, &unreadable.0) {
                    log::event(
                        "engine.usage_state_db_unreadable",
                        &[("kind", &unreadable.0)],
                    );
                }
                SecretRead::Denied
            }
        }
    }

    fn program_found(&self, program: &str) -> bool {
        executable(program).is_some()
    }

    fn opencode_credential(&self, database: &Path, integration: &str) -> StoredCredential {
        opencode_read(
            soyeht_usage::foreign_db::opencode_credential(database, integration),
            StoredCredential::Unreadable,
        )
    }

    fn opencode_usage(
        &self,
        database: &Path,
        since_ms: i64,
        provider: Option<&str>,
    ) -> LocalUsageRead {
        opencode_read(
            soyeht_usage::foreign_db::opencode_usage(database, since_ms, provider),
            LocalUsageRead::Unreadable,
        )
    }

    fn json_rpc(&self, request: &RpcRequest) -> RpcRun {
        let Some(program) = executable(&request.program) else {
            return RpcRun::ToolNotFound;
        };
        process::json_rpc(&program, &launch_path(), request)
    }

    fn run_terminal(
        &self,
        request: &TerminalRequest,
        driver: &mut dyn TerminalDriver,
    ) -> TerminalRun {
        let Some(program) = executable(&request.program) else {
            return TerminalRun::ToolNotFound;
        };
        // The scratch directory must be Soyeht's own, inside the temporary directory.
        if !request.workdir.starts_with(self.temp_dir()) {
            return TerminalRun::Incomplete;
        }
        terminal::run(&program, &launch_path(), request, driver)
    }
}

/// An OpenCode database read: its value, or `unreadable` after logging why, once per streak.
fn opencode_read<T>(read: Result<T, soyeht_usage::foreign_db::Unreadable>, unreadable: T) -> T {
    match read {
        Ok(value) => {
            recovered(&OPENCODE_DB);
            value
        }
        Err(failure) => {
            if first_failure(&OPENCODE_DB, &failure.0) {
                log::event(
                    "engine.usage_opencode_db_unreadable",
                    &[("kind", &failure.0)],
                );
            }
            unreadable
        }
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
            SystemHost.read_file(&path, 10),
            FileRead::Bytes(b"0123456789".to_vec())
        );
        assert_eq!(SystemHost.read_file(&path, 9), FileRead::Unreadable);
        assert_eq!(
            SystemHost.read_file(&path.with_extension("absent"), 9),
            FileRead::Missing
        );
        fs::remove_file(path).unwrap();
    }
}
