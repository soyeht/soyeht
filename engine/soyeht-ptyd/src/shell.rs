//! The command a terminal pane runs when its profile names none.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// The shell a new terminal pane starts, as an argument vector.
///
/// macOS always starts the system `zsh` as a login shell. Windows starts the command
/// interpreter named by `COMSPEC` (`cmd.exe` when unset), which has no login mode. Other
/// systems start the person's `$SHELL` when it names an existing absolute path (a supervisor
/// started by a desktop session may not have one), then the shell the account names, then `bash`,
/// then the POSIX `sh`, always as a login shell.
pub fn default_login_shell() -> Vec<String> {
    if cfg!(target_os = "macos") {
        return vec!["/bin/zsh".into(), "-l".into()];
    }
    if cfg!(windows) {
        return vec![std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())];
    }
    vec![
        unix_shell(std::env::var_os("SHELL"), account_shell),
        "-l".into(),
    ]
}

/// Reads what [`default_login_shell`] needs from the system now, so that the first terminal does
/// not wait for it. The engine calls it at start, before its state lock exists: a terminal is
/// started under that lock, and the user database may be a directory service.
pub fn warm_default_login_shell() {
    drop(default_login_shell());
}

/// The shell the user database names for this account, read once per process: the lookup may
/// reach a directory service, and a terminal is started under the engine's state lock. A failed
/// lookup names none, which is correct here: the fallbacks after it start a shell all the same.
fn account_shell() -> Option<PathBuf> {
    static ACCOUNT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ACCOUNT
        .get_or_init(|| soyeht_platform::user::login_shell().unwrap_or(None))
        .clone()
}

/// An account shell that exists to refuse a login: starting it ends the pane at once.
fn refuses_login(shell: &Path) -> bool {
    matches!(
        shell.file_name().and_then(|name| name.to_str()),
        Some("nologin" | "false")
    )
}

/// `$SHELL`, else the account's shell (asked only when `$SHELL` is not usable), each only when
/// it names an existing absolute path, else the first `bash` there is, else the POSIX `sh`.
/// The account's shell is not one that refuses logins.
fn unix_shell(environment: Option<OsString>, account: impl FnOnce() -> Option<PathBuf>) -> String {
    let usable = |shell: PathBuf| {
        (shell.is_absolute() && shell.is_file())
            .then(|| shell.to_str().map(str::to_owned))
            .flatten()
    };
    environment
        .map(PathBuf::from)
        .and_then(usable)
        .or_else(|| {
            account()
                .filter(|shell| !refuses_login(shell))
                .and_then(usable)
        })
        .unwrap_or_else(|| {
            ["/bin/bash", "/usr/bin/bash"]
                .into_iter()
                .find(|candidate| Path::new(candidate).is_file())
                .unwrap_or("/bin/sh")
                .into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_shell_exists_and_is_a_login_shell_where_the_system_has_one() {
        let command = default_login_shell();
        assert!(!command.is_empty());
        if cfg!(target_os = "macos") {
            assert_eq!(command, ["/bin/zsh", "-l"]);
        } else if cfg!(windows) {
            assert_eq!(command.len(), 1);
        } else {
            assert_eq!(command.last().map(String::as_str), Some("-l"));
            assert!(Path::new(&command[0]).is_file());
        }
    }

    #[test]
    fn an_unset_shell_variable_falls_back_to_the_account_then_to_bash() {
        let bash = Path::new("/bin/bash");
        if !bash.is_file() {
            return;
        }
        let account = || Some(PathBuf::from("/bin/bash"));
        assert_eq!(unix_shell(None, account), "/bin/bash");
        assert_eq!(unix_shell(Some(OsString::new()), account), "/bin/bash");
        // A variable that names no usable file does not hide the account.
        assert_eq!(unix_shell(Some("zsh".into()), account), "/bin/bash");
        // Neither names one: the first bash that exists.
        assert!(unix_shell(None, || Some("/nowhere/fish".into())).ends_with("bash"));
        assert!(unix_shell(None, || None).ends_with("bash"));
    }

    #[test]
    fn the_shell_variable_beats_the_account_and_the_account_is_not_asked() {
        if !Path::new("/bin/sh").is_file() {
            return;
        }
        let must_not_be_asked = || -> Option<PathBuf> { panic!("the lookup reads a directory") };
        assert_eq!(
            unix_shell(Some("/bin/sh".into()), must_not_be_asked),
            "/bin/sh"
        );
    }

    #[test]
    fn an_account_shell_that_refuses_logins_is_not_started() {
        assert!(refuses_login(Path::new("/usr/sbin/nologin")));
        assert!(refuses_login(Path::new("/sbin/nologin")));
        assert!(refuses_login(Path::new("/bin/false")));
        assert!(!refuses_login(Path::new("/bin/bash")));
        // The same through the choice, on a host that has one of them.
        let refusing = [
            "/usr/sbin/nologin",
            "/sbin/nologin",
            "/bin/false",
            "/usr/bin/false",
        ]
        .into_iter()
        .find(|path| Path::new(path).is_file());
        if let Some(refusing) = refusing {
            let chosen = unix_shell(None, || Some(refusing.into()));
            assert!(chosen.ends_with("bash") || chosen == "/bin/sh", "{chosen}");
        }
    }

    #[test]
    fn the_account_is_looked_up_once_per_process() {
        // The same value is returned from the cell on every call; a changed user database is
        // not read again.
        assert_eq!(account_shell(), account_shell());
    }
}
