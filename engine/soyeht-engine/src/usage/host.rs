//! The engine's [`Host`]: the real file system, keychain, HTTPS, browser cookies and pseudo
//! terminal behind the usage sources.

use super::{process, terminal};
use crate::{
    agents::{environment::launch_path, registry::executable},
    streak::{Streak, first_failure, recovered},
};
use soyeht_platform::{
    fs::ensure_private_dir,
    log::{self, Failure as _},
};
use soyeht_usage::host::{
    CookieRead, FileRead, Host, HttpRequest, HttpResult, Platform, RpcRequest, RpcRun, SecretRead,
    TerminalDriver, TerminalRequest, TerminalRun,
};
use soyeht_usage::host::{LocalUsageRead, StoredCredential};
use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// A state database that cannot be read, counted across refreshes: it is read on each one.
static STATE_DB: Mutex<Streak> = Mutex::new(Streak::once());

/// OpenCode's database when it cannot be read, counted across refreshes: it is read on each one.
static OPENCODE_DB: Mutex<Streak> = Mutex::new(Streak::once());

/// The folder of the engine's data directory that holds the scratch directories of terminal runs.
const SCRATCH_DIRECTORY: &str = "usage-scratch";

/// A scratch directory that could not be made owner-only, counted across refreshes: a source
/// that needs a terminal asks for it on each one.
static SCRATCH_ROOT: Mutex<Streak> = Mutex::new(Streak::once());

/// The real host: the machine's files, keychain, HTTPS, browser cookies and pseudo terminal.
pub struct SystemHost {
    /// Where terminal runs get their working directories: a directory inside the engine's data
    /// directory that only the user reaches, not the temporary directory every user shares.
    scratch: PathBuf,
}

impl SystemHost {
    pub fn new(data_directory: &Path) -> Self {
        Self {
            scratch: data_directory.join(SCRATCH_DIRECTORY),
        }
    }

    /// The scratch directory, made owner-only when missing and refused when a link, a file or
    /// another user's directory is in its place. It is canonical, because Claude Code names a
    /// project after its real path.
    fn private_scratch(&self) -> io::Result<PathBuf> {
        ensure_private_dir(&self.scratch)?;
        soyeht_platform::fs::canonicalize(&self.scratch)
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
        // ignored: an unset or non-Unicode variable counts as not set
        std::env::var(name).ok()
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::home_dir().filter(|home| home.is_absolute())
    }

    /// The owner-only scratch directory of this engine. When it cannot be made the name is still
    /// returned, and [`run_terminal`](Host::run_terminal) refuses to run in it.
    fn temp_dir(&self) -> PathBuf {
        // ignored: `run_terminal` meets the same error, logs it and refuses to run.
        self.private_scratch()
            .unwrap_or_else(|_| self.scratch.clone())
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
        // The scratch directory must be Soyeht's own, inside the owner-only directory made for it.
        let scratch = match self.private_scratch() {
            Ok(scratch) => {
                recovered(&SCRATCH_ROOT);
                scratch
            }
            Err(error) => {
                if first_failure(&SCRATCH_ROOT, &error.failure_kind()) {
                    log::failure("engine.usage_scratch_unavailable", &error);
                }
                return TerminalRun::Incomplete;
            }
        };
        if !request.workdir.starts_with(scratch) {
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
    use soyeht_platform::fs::{dir_is_owner_only, symlink};
    use uuid::Uuid;

    fn data_directory() -> PathBuf {
        let directory = std::env::temp_dir().join(format!("sn-usage-host-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn files_over_their_bound_are_unreadable() {
        let directory = data_directory();
        let host = SystemHost::new(&directory);
        let path = directory.join("bounded");
        fs::write(&path, b"0123456789").unwrap();
        assert_eq!(
            host.read_file(&path, 10),
            FileRead::Bytes(b"0123456789".to_vec())
        );
        assert_eq!(host.read_file(&path, 9), FileRead::Unreadable);
        assert_eq!(
            host.read_file(&path.with_extension("absent"), 9),
            FileRead::Missing
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn terminal_runs_get_an_owner_only_directory_that_is_not_the_shared_temporary_one() {
        let directory = data_directory();
        let scratch = SystemHost::new(&directory).temp_dir();
        assert!(dir_is_owner_only(&scratch).unwrap());
        let shared = std::env::temp_dir();
        assert_ne!(
            scratch,
            soyeht_platform::fs::canonicalize(&shared).unwrap_or(shared)
        );
        assert!(scratch.starts_with(soyeht_platform::fs::canonicalize(&directory).unwrap()));
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_directory_others_can_reach_is_made_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = data_directory();
        let host = SystemHost::new(&directory);
        fs::create_dir(&host.scratch).unwrap();
        fs::set_permissions(&host.scratch, fs::Permissions::from_mode(0o777)).unwrap();
        let scratch = host.private_scratch().unwrap();
        assert!(dir_is_owner_only(&scratch).unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_link_in_place_of_the_scratch_directory_is_refused_and_not_followed() {
        let directory = data_directory();
        let host = SystemHost::new(&directory);
        let elsewhere = directory.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        symlink(&elsewhere, &host.scratch).unwrap();
        assert!(host.private_scratch().is_err());
        // The name is still given, but a terminal run in it is refused (`run_terminal`).
        assert_eq!(host.temp_dir(), host.scratch);
        assert_eq!(
            fs::read_dir(&elsewhere).unwrap().count(),
            0,
            "nothing is made behind the link"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
