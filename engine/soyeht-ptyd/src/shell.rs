//! The command a terminal pane runs when its profile names none.

use std::path::{Path, PathBuf};

/// The shell a new terminal pane starts, as an argument vector.
///
/// macOS always starts the system `zsh` as a login shell. Windows starts the command
/// interpreter named by `COMSPEC` (`cmd.exe` when unset), which has no login mode. Other
/// systems start the person's `$SHELL` when it names an existing absolute path, then `bash`,
/// then the POSIX `sh`, always as a login shell.
pub fn default_login_shell() -> Vec<String> {
    if cfg!(target_os = "macos") {
        return vec!["/bin/zsh".into(), "-l".into()];
    }
    if cfg!(windows) {
        return vec![std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())];
    }
    let chosen = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|shell| shell.is_absolute() && shell.is_file())
        .and_then(|shell| shell.to_str().map(str::to_owned));
    let shell = chosen.unwrap_or_else(|| {
        ["/bin/bash", "/usr/bin/bash"]
            .into_iter()
            .find(|candidate| Path::new(candidate).is_file())
            .unwrap_or("/bin/sh")
            .into()
    });
    vec![shell, "-l".into()]
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
}
