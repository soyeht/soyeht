//! Asks the person's login shell for its `PATH`.
//!
//! An app started from the desktop does not inherit the `PATH` a terminal has, so the engine
//! asks an interactive login shell for it. Only Unix has such shells.

use super::LaunchPathIssue;
use std::{
    ffi::OsString,
    io::{self, Read},
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PATH_PROBE_BYTES: usize = 64 * 1024;
const PATH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PATH_START: u8 = 0x1e;
const PATH_END: u8 = 0x1f;
const POSIX_PATH_COMMAND: &str = "printf '\\036%s\\037' \"$PATH\"";
const FISH_PATH_COMMAND: &str = "printf '\\036%s\\037' (string join : $PATH)";

#[derive(Debug, Eq, PartialEq)]
struct ShellProbe {
    executable: PathBuf,
    command: &'static str,
}

fn probe_output(bytes: &[u8]) -> Option<String> {
    let start = bytes.iter().rposition(|byte| *byte == PATH_START)? + 1;
    let end = bytes[start..].iter().position(|byte| *byte == PATH_END)? + start;
    let path = std::str::from_utf8(&bytes[start..end]).ok()?;
    (!path.is_empty() && !path.contains('\0')).then(|| path.into())
}

/// The person's shell: `$SHELL`, which the login sets, else the one the account names. A program
/// the desktop session started (a GNOME or KDE launcher, a service) may not have `$SHELL`.
fn supported_shell() -> Result<ShellProbe, LaunchPathIssue> {
    supported_shell_of(
        std::env::var_os("SHELL"),
        soyeht_platform::user::login_shell,
    )
}

fn supported_shell_of(
    environment: Option<OsString>,
    account: impl FnOnce() -> io::Result<Option<PathBuf>>,
) -> Result<ShellProbe, LaunchPathIssue> {
    let shell = match environment.filter(|value| !value.is_empty()) {
        Some(shell) => PathBuf::from(shell),
        // A lookup that fails leaves the engine as it is without a shell: its own `PATH`.
        None => account()
            .map_err(|_| LaunchPathIssue::MissingShell)?
            .ok_or(LaunchPathIssue::MissingShell)?,
    };
    let command = shell_command(&shell).ok_or(LaunchPathIssue::UnsupportedShell)?;
    let metadata = shell
        .metadata()
        .map_err(|_| LaunchPathIssue::UnavailableShell)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(LaunchPathIssue::UnavailableShell);
    }
    Ok(ShellProbe {
        executable: shell,
        command,
    })
}

fn shell_command(shell: &Path) -> Option<&'static str> {
    let name = shell
        .is_absolute()
        .then(|| shell.file_name()?.to_str())
        .flatten()?;
    match name {
        "bash" | "zsh" => Some(POSIX_PATH_COMMAND),
        "fish" => Some(FISH_PATH_COMMAND),
        _ => None,
    }
}

fn set_nonblocking(stdout: &ChildStdout) -> io::Result<()> {
    let descriptor = stdout.as_raw_fd();
    // SAFETY: descriptor is owned by `stdout` for this call, and F_GETFL does
    // not mutate memory. The returned flags are passed back to F_SETFL with
    // O_NONBLOCK added; both calls are checked before the descriptor is read.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains alive through `stdout`, and F_SETFL only
    // changes status flags for that descriptor. The return value is checked.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_available(stdout: &mut ChildStdout, bytes: &mut Vec<u8>) -> io::Result<bool> {
    let mut buffer = [0_u8; 4 * 1024];
    loop {
        match stdout.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.len() > PATH_PROBE_BYTES {
                    return Ok(false);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn run_probe_command(
    shell: &Path,
    command: &str,
    timeout: Duration,
) -> Result<String, LaunchPathIssue> {
    let mut probe = Command::new(shell);
    probe
        .args(["-l", "-i", "-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Not through `soyeht_process::run`: the probe reads its pipe by hand, so it can end at
    // the first complete answer. Its start still queues with every other.
    let mut child = soyeht_process::starting("shell", || probe.spawn())
        .map_err(|_| LaunchPathIssue::ProbeFailed)?
        .map_err(|_| LaunchPathIssue::ProbeFailed)?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err(abandon(&mut child, LaunchPathIssue::ProbeFailed));
    };
    if set_nonblocking(&stdout).is_err() {
        return Err(abandon(&mut child, LaunchPathIssue::ProbeFailed));
    }
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut exited = None;
    let mut eof = false;
    while Instant::now() < deadline {
        match read_available(&mut stdout, &mut bytes) {
            Ok(done) => eof |= done,
            // The typed `ProbeFailed` issue is the report; the shell's output is not logged.
            Err(_) => return Err(abandon(&mut child, LaunchPathIssue::ProbeFailed)),
        }
        if bytes.len() > PATH_PROBE_BYTES {
            return Err(abandon(&mut child, LaunchPathIssue::OutputTooLarge));
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(status) => exited = status,
                // The typed `ProbeFailed` issue is the report.
                Err(_) => return Err(abandon(&mut child, LaunchPathIssue::ProbeFailed)),
            }
        }
        if eof && exited.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if exited.is_none() || !eof {
        return Err(abandon(&mut child, LaunchPathIssue::ProbeTimedOut));
    }
    if !exited.is_some_and(|status| status.success()) {
        return Err(LaunchPathIssue::ProbeFailed);
    }
    probe_output(&bytes).ok_or(LaunchPathIssue::InvalidOutput)
}

/// Stops a probe that already failed. Best effort: the returned issue already says why
/// the probe failed, and a kill or reap error would add nothing to it.
fn abandon(child: &mut std::process::Child, issue: LaunchPathIssue) -> LaunchPathIssue {
    let _ = child.kill();
    let _ = child.wait();
    issue
}

/// The `PATH` an interactive login shell of the person has.
pub(super) fn login_shell_path() -> Result<String, LaunchPathIssue> {
    let shell = supported_shell()?;
    run_probe_command(&shell.executable, shell.command, PATH_PROBE_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_probe_ignores_shell_banner_and_requires_bounded_markers() {
        assert_eq!(
            probe_output(b"banner\n\x1e/usr/local/bin:/usr/bin\x1ftrailer"),
            Some("/usr/local/bin:/usr/bin".into())
        );
        assert_eq!(probe_output(b"banner only"), None);
        assert_eq!(probe_output(b"\x1e\x1f"), None);
    }

    #[test]
    fn a_background_stdout_holder_cannot_extend_the_probe_deadline() {
        let started = Instant::now();
        assert_eq!(
            run_probe_command(
                Path::new("/bin/bash"),
                "printf '\\036%s\\037' \"$PATH\"; sleep 1 &",
                Duration::from_millis(100)
            ),
            Err(LaunchPathIssue::ProbeTimedOut)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    fn account_names(shell: &'static str) -> impl FnOnce() -> io::Result<Option<PathBuf>> {
        move || Ok(Some(PathBuf::from(shell)))
    }

    fn account_must_not_be_asked() -> io::Result<Option<PathBuf>> {
        panic!("the environment names a shell")
    }

    #[test]
    fn a_missing_shell_variable_falls_back_to_the_account_shell() {
        let bash = Path::new("/bin/bash");
        if !bash.is_file() {
            return;
        }
        for unset in [None, Some(OsString::new())] {
            let probe = supported_shell_of(unset, account_names("/bin/bash")).unwrap();
            assert_eq!(probe.executable, bash);
            assert_eq!(probe.command, POSIX_PATH_COMMAND);
        }
    }

    #[test]
    fn the_shell_variable_beats_the_account_and_is_checked_as_before() {
        assert_eq!(
            supported_shell_of(Some("/bin/tcsh".into()), account_must_not_be_asked).unwrap_err(),
            LaunchPathIssue::UnsupportedShell
        );
        assert_eq!(
            supported_shell_of(Some("/nowhere/zsh".into()), account_must_not_be_asked).unwrap_err(),
            LaunchPathIssue::UnavailableShell
        );
    }

    #[test]
    fn no_shell_anywhere_is_a_missing_shell() {
        assert_eq!(
            supported_shell_of(None, || Ok(None)).unwrap_err(),
            LaunchPathIssue::MissingShell
        );
        assert_eq!(
            supported_shell_of(None, || Err(io::ErrorKind::Other.into())).unwrap_err(),
            LaunchPathIssue::MissingShell
        );
        // An account whose shell the probe cannot use is that, not a missing shell.
        assert_eq!(
            supported_shell_of(None, account_names("/usr/sbin/nologin")).unwrap_err(),
            LaunchPathIssue::UnsupportedShell
        );
    }

    #[test]
    fn supported_login_shells_use_their_native_path_expression() {
        assert_eq!(
            shell_command(Path::new("/bin/zsh")),
            Some(POSIX_PATH_COMMAND)
        );
        assert_eq!(
            shell_command(Path::new("/bin/bash")),
            Some(POSIX_PATH_COMMAND)
        );
        assert_eq!(
            shell_command(Path::new("/opt/homebrew/bin/fish")),
            Some(FISH_PATH_COMMAND)
        );
        assert_eq!(shell_command(Path::new("zsh")), None);
        assert_eq!(shell_command(Path::new("/bin/tcsh")), None);
    }
}
