use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{ChildStdout, Command, Stdio},
    sync::{OnceLock, RwLock},
    thread,
    time::{Duration, Instant},
};

const PATH_PROBE_BYTES: usize = 64 * 1024;
const PATH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PATH_START: u8 = 0x1e;
const PATH_END: u8 = 0x1f;
const POSIX_PATH_COMMAND: &str = "printf '\\036%s\\037' \"$PATH\"";
const FISH_PATH_COMMAND: &str = "printf '\\036%s\\037' (string join : $PATH)";
static LAUNCH_PATH: OnceLock<RwLock<Option<LaunchPath>>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchPath {
    pub value: String,
    pub issue: Option<LaunchPathIssue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchPathIssue {
    MissingShell,
    UnsupportedShell,
    UnavailableShell,
    ProbeFailed,
    ProbeTimedOut,
    OutputTooLarge,
    InvalidOutput,
}

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

fn supported_shell() -> Result<ShellProbe, LaunchPathIssue> {
    let shell = std::env::var_os("SHELL")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(LaunchPathIssue::MissingShell)?;
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
    let mut buffer = [0_u8; 4096];
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
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| LaunchPathIssue::ProbeFailed)?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(LaunchPathIssue::ProbeFailed);
    };
    if set_nonblocking(&stdout).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(LaunchPathIssue::ProbeFailed);
    }
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut exited = None;
    let mut eof = false;
    while Instant::now() < deadline {
        eof |= read_available(&mut stdout, &mut bytes).map_err(|_| LaunchPathIssue::ProbeFailed)?;
        if bytes.len() > PATH_PROBE_BYTES {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LaunchPathIssue::OutputTooLarge);
        }
        if exited.is_none() {
            exited = child.try_wait().map_err(|_| LaunchPathIssue::ProbeFailed)?;
        }
        if eof && exited.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if exited.is_none() || !eof {
        let _ = child.kill();
        let _ = child.wait();
        return Err(LaunchPathIssue::ProbeTimedOut);
    }
    if !exited.is_some_and(|status| status.success()) {
        return Err(LaunchPathIssue::ProbeFailed);
    }
    probe_output(&bytes).ok_or(LaunchPathIssue::InvalidOutput)
}

fn run_probe(shell: &ShellProbe, timeout: Duration) -> Result<String, LaunchPathIssue> {
    run_probe_command(&shell.executable, shell.command, timeout)
}

fn resolve_launch_path() -> LaunchPath {
    let interactive = supported_shell().and_then(|shell| run_probe(&shell, PATH_PROBE_TIMEOUT));
    match interactive {
        Ok(value) => LaunchPath { value, issue: None },
        Err(issue) => LaunchPath {
            value: std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            issue: Some(issue),
        },
    }
}

pub fn launch_path_resolution() -> LaunchPath {
    let cache = LAUNCH_PATH.get_or_init(|| RwLock::new(None));
    if let Some(path) = cache.read().ok().and_then(|value| value.clone()) {
        return path;
    }
    let path = resolve_launch_path();
    if let Ok(mut value) = cache.write() {
        *value = Some(path.clone());
    }
    path
}

pub fn launch_path() -> String {
    launch_path_resolution().value
}

pub fn invalidate_launch_path() {
    if let Some(cache) = LAUNCH_PATH.get()
        && let Ok(mut value) = cache.write()
    {
        *value = None;
    }
}

pub fn launch_environment() -> BTreeMap<String, String> {
    BTreeMap::from([("PATH".into(), launch_path())])
}

pub fn candidate_directories(recipe_directories: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut seen = BTreeSet::new();
    for paths in [Some(launch_path().into()), std::env::var_os("PATH")] {
        let split = paths
            .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .unwrap_or_default();
        for directory in split {
            if seen.insert(directory.clone()) {
                directories.push(directory);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let directory = home.join(".local/bin");
        if seen.insert(directory.clone()) {
            directories.push(directory);
        }
    }
    let homebrew = PathBuf::from("/opt/homebrew/bin");
    if seen.insert(homebrew.clone()) {
        directories.push(homebrew);
    }
    for directory in recipe_directories {
        if seen.insert(directory.clone()) {
            directories.push(directory.clone());
        }
    }
    directories
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
    fn launch_environment_carries_the_resolved_path_to_the_child() {
        let environment = launch_environment();
        assert_eq!(environment.get("PATH"), Some(&launch_path()));
        assert!(!environment.contains_key("HOME"));
    }

    #[test]
    fn a_background_stdout_holder_cannot_extend_the_probe_deadline() {
        let started = Instant::now();
        assert_eq!(
            run_probe_command(
                Path::new("/bin/zsh"),
                "printf '\\036%s\\037' \"$PATH\"; sleep 1 &",
                Duration::from_millis(100)
            ),
            Err(LaunchPathIssue::ProbeTimedOut)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
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

    #[test]
    fn recipe_directories_follow_the_shared_fallbacks() {
        let recipe = PathBuf::from("/tmp/recipe-bin");
        let directories = candidate_directories(std::slice::from_ref(&recipe));
        assert_eq!(directories.last(), Some(&recipe));
        assert!(!directories.iter().any(|path| path.ends_with(".grok/bin")));
    }
}
