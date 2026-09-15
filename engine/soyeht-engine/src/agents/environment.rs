use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{OnceLock, RwLock},
    thread,
    time::{Duration, Instant},
};

const PATH_PROBE_BYTES: usize = 64 * 1024;
const PATH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PATH_START: u8 = 0x1e;
const PATH_END: u8 = 0x1f;
static LAUNCH_PATH: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn probe_output(bytes: &[u8]) -> Option<String> {
    let start = bytes.iter().rposition(|byte| *byte == PATH_START)? + 1;
    let end = bytes[start..].iter().position(|byte| *byte == PATH_END)? + start;
    let path = std::str::from_utf8(&bytes[start..end]).ok()?;
    (!path.is_empty() && !path.contains('\0')).then(|| path.into())
}

fn read_probe(output: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = output
        .take(PATH_PROBE_BYTES as u64 + 1)
        .read_to_end(&mut bytes);
    bytes
}

fn interactive_path() -> Option<String> {
    let mut child = Command::new("/bin/zsh")
        .args(["-lic", "printf '\\036%s\\037' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || read_probe(stdout));
    let deadline = Instant::now() + PATH_PROBE_TIMEOUT;
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader.join().ok()?;
    (success && bytes.len() <= PATH_PROBE_BYTES)
        .then(|| probe_output(&bytes))
        .flatten()
}

pub fn launch_path() -> String {
    let cache = LAUNCH_PATH.get_or_init(|| RwLock::new(None));
    if let Some(path) = cache.read().ok().and_then(|value| value.clone()) {
        return path;
    }
    let path = interactive_path()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "/usr/bin:/bin".into());
    if let Ok(mut value) = cache.write() {
        *value = Some(path.clone());
    }
    path
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

pub fn candidate_directories() -> Vec<PathBuf> {
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
        for relative in [".local/bin", ".grok/bin"] {
            let directory = home.join(relative);
            if seen.insert(directory.clone()) {
                directories.push(directory);
            }
        }
    }
    let homebrew = PathBuf::from("/opt/homebrew/bin");
    if seen.insert(homebrew.clone()) {
        directories.push(homebrew);
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
}
