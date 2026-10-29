//! The command a terminal pane runs when its profile names none.

/// The shell a new terminal pane starts, as an argument vector.
///
/// macOS always starts the system `zsh` as a login shell. Windows starts the command
/// interpreter named by `COMSPEC` (`cmd.exe` when unset), which has no login mode. Other
/// systems start the person's preferred shell (`soyeht_platform::user::preferred_shell`: the
/// usable `$SHELL`, then the account's, then `bash`, then the POSIX `sh`), always as a login
/// shell.
pub fn default_login_shell() -> Vec<String> {
    if cfg!(target_os = "macos") {
        return vec!["/bin/zsh".into(), "-l".into()];
    }
    if cfg!(windows) {
        return vec![std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())];
    }
    // Only Windows has no preferred shell, and it returned above; a path that is not text cannot
    // be an argument, and the POSIX shell is always there.
    let shell = soyeht_platform::user::preferred_shell().unwrap_or_else(|_| "/bin/sh".into());
    vec![shell.to_str().unwrap_or("/bin/sh").to_owned(), "-l".into()]
}

/// Reads what the engine's shell choices need from the system now, so that neither the first
/// terminal nor the first probe of the person's `PATH` waits for it. The engine calls it at
/// start, before its state lock exists: a terminal is started under that lock, and the user
/// database may be a directory service.
pub fn warm_default_login_shell() {
    drop(default_login_shell());
    // macOS starts a fixed shell above, but the engine's probe of the login shell's `PATH`
    // asks for the preferred one there too.
    drop(soyeht_platform::user::preferred_shell());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
}
