//! Asks the person's login shell for its `PATH`.
//!
//! An app started from the desktop does not inherit the `PATH` a terminal has, so the engine
//! asks an interactive login shell for it. Only Unix has such shells.

use super::LaunchPathIssue;
use std::{
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

/// The person's shell, by the rule every caller shares (`soyeht_platform::user::preferred_shell`),
/// checked for what the probe needs: one it can ask, that exists. A host with no shell at all
/// leaves the engine as it is without one: its own `PATH`.
fn supported_shell() -> Result<ShellProbe, LaunchPathIssue> {
    let shell =
        soyeht_platform::user::preferred_shell().map_err(|_| LaunchPathIssue::MissingShell)?;
    probe_of(shell)
}

fn probe_of(shell: PathBuf) -> Result<ShellProbe, LaunchPathIssue> {
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

    #[test]
    fn a_shell_the_probe_can_ask_is_accepted() {
        let bash = Path::new("/bin/bash");
        if !bash.is_file() {
            return;
        }
        let probe = probe_of(bash.into()).unwrap();
        assert_eq!(probe.executable, bash);
        assert_eq!(probe.command, POSIX_PATH_COMMAND);
    }

    #[test]
    fn a_shell_the_probe_cannot_ask_or_cannot_find_is_a_named_issue() {
        assert_eq!(
            probe_of("/bin/tcsh".into()).unwrap_err(),
            LaunchPathIssue::UnsupportedShell
        );
        assert_eq!(
            probe_of("/nowhere/zsh".into()).unwrap_err(),
            LaunchPathIssue::UnavailableShell
        );
        // The last fallback of the shared rule is not one the probe knows how to ask.
        assert_eq!(
            probe_of("/bin/sh".into()).unwrap_err(),
            LaunchPathIssue::UnsupportedShell
        );
    }

    #[test]
    fn the_probe_is_offered_the_preferred_shell() {
        // One rule: this host's preferred shell is what the probe is offered. Terminals start the
        // same shell on Linux; on macOS they always start `zsh`, which the supervisor decides.
        let preferred = soyeht_platform::user::preferred_shell().unwrap();
        match supported_shell() {
            Ok(probe) => assert_eq!(probe.executable, preferred),
            Err(issue) => assert!(
                matches!(
                    issue,
                    LaunchPathIssue::UnsupportedShell | LaunchPathIssue::UnavailableShell
                ),
                "{issue:?}"
            ),
        }
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
