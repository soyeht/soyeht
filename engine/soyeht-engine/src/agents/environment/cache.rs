//! The launch `PATH`, resolved once and shared by every launch of this engine.
//!
//! Asking the person's interactive login shell takes seconds on a busy machine (it reads the
//! person's startup files), and the engine cannot listen, nor restore its agents, until it
//! knows the answer. So the engine remembers the last answer in its data directory, uses it
//! at once on the next start and asks the shell again in the background.
//! Only a caller that finds no answer at all waits for the shell.

use super::LaunchPath;
use soyeht_platform::fs::OpenOptionsPrivateExt;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
    thread,
};

const FILE: &str = "launch-path";
/// A `PATH` is a few kilobytes; the shell probe itself refuses more than this.
const MAX_BYTES: u64 = 64 * 1024;

struct State {
    resolved: Option<LaunchPath>,
    /// A thread is asking the shell; it stores the answer and wakes the waiters.
    probing: bool,
    file: Option<PathBuf>,
}

pub(super) struct Cache {
    state: Mutex<State>,
    answered: Condvar,
}

/// Asks the person's shell.
type Probe = fn() -> LaunchPath;

impl Cache {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                resolved: None,
                probing: false,
                file: None,
            }),
            answered: Condvar::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The state stays consistent if a holder panics: every update is one assignment.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The answer, asking the shell now only when nobody has and nobody is asking.
    pub(super) fn resolution(&self, probe: Probe) -> LaunchPath {
        let mut state = self.state();
        loop {
            if let Some(path) = &state.resolved {
                return path.clone();
            }
            if !state.probing {
                break;
            }
            state = self
                .answered
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.probing = true;
        drop(state);
        self.answer(probe(), None)
    }

    /// Asks the shell again and returns the new answer. Only this caller waits: until the
    /// shell replies, every other caller keeps the previous answer, so a thread that holds
    /// the Core lock never waits on a shell (ADR 0079). A refresh that finds the shell
    /// already being asked waits for that answer instead of asking again.
    ///
    /// Call it from a thread that holds no lock.
    pub(super) fn refresh(&self, probe: Probe) -> LaunchPath {
        let mut state = self.state();
        if state.probing {
            while state.probing {
                state = self
                    .answered
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if let Some(path) = &state.resolved {
                return path.clone();
            }
        }
        state.probing = true;
        let previous = state.resolved.as_ref().map(|path| path.value.clone());
        drop(state);
        self.answer(probe(), previous)
    }

    /// Starts with the answer remembered in `directory`, if any, and refreshes it on a thread.
    /// Does nothing once an answer or a refresh exists.
    pub(super) fn warm(&'static self, directory: &Path, probe: Probe) {
        let file = directory.join(FILE);
        let remembered = remembered(&file);
        let mut state = self.state();
        state.file = Some(file);
        if state.resolved.is_some() || state.probing {
            return;
        }
        state.probing = true;
        state.resolved = remembered
            .clone()
            .map(|value| LaunchPath { value, issue: None });
        let spawned = thread::Builder::new()
            .name("launch-path".into())
            .spawn(move || {
                self.answer(probe(), remembered);
            });
        if spawned.is_err() {
            // The first caller asks the shell itself.
            state.probing = false;
        }
    }

    /// Stores what the shell said, wakes the waiters and remembers a good answer for the next
    /// start.
    ///
    /// A shell that failed keeps the `PATH` remembered from the last start, with the failure
    /// reported beside it.
    fn answer(&self, probed: LaunchPath, remembered: Option<String>) -> LaunchPath {
        let (path, file) = {
            let mut state = self.state();
            let path = match (probed.issue, remembered.as_ref()) {
                (Some(issue), Some(value)) => LaunchPath {
                    value: value.clone(),
                    issue: Some(issue),
                },
                _ => probed,
            };
            state.resolved = Some(path.clone());
            state.probing = false;
            self.answered.notify_all();
            (path, state.file.clone())
        };
        if path.issue.is_none()
            && remembered.as_deref() != Some(path.value.as_str())
            && let Some(file) = file
            && let Err(error) = remember(&file, &path.value)
        {
            soyeht_platform::log::failure("engine.launch_path_not_saved", &error);
        }
        path
    }
}

fn remembered(file: &Path) -> Option<String> {
    if !cfg!(unix) {
        // Elsewhere the engine's own `PATH` is the answer, and it is free to read.
        return None;
    }
    // A missing, unreadable or malformed file is no memory: the shell is asked.
    match read(file) {
        Ok(value) if !value.is_empty() && !value.contains('\0') => Some(value),
        _ => None,
    }
}

fn read(file: &Path) -> std::io::Result<String> {
    let mut value = String::new();
    let mut reader = OpenOptions::new()
        .read(true)
        .open_no_follow(file)?
        .take(MAX_BYTES + 1);
    reader.read_to_string(&mut value)?;
    if value.len() as u64 > MAX_BYTES {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(value)
}

fn remember(file: &Path, value: &str) -> std::io::Result<()> {
    let temporary = file.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).owner_only();
    options
        .open_no_follow(&temporary)?
        .write_all(value.as_bytes())?;
    fs::rename(&temporary, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::Duration,
    };

    fn directory() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("soyeht-launch-path-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_remembered_path_is_read_back_and_bad_files_are_ignored() {
        let directory = directory();
        let file = directory.join(FILE);
        assert_eq!(remembered(&file), None);
        remember(&file, "/opt/bin:/usr/bin").unwrap();
        assert_eq!(remembered(&file).as_deref(), Some("/opt/bin:/usr/bin"));
        fs::write(&file, "").unwrap();
        assert_eq!(remembered(&file), None);
        fs::write(&file, b"/usr/bin\0/bin").unwrap();
        assert_eq!(remembered(&file), None);
        fs::write(&file, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(remembered(&file), None);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_remembered_path_is_private_and_a_link_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let directory = directory();
        let file = directory.join(FILE);
        remember(&file, "/usr/bin").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o077, 0);
        fs::remove_file(&file).unwrap();
        let target = directory.join("elsewhere");
        fs::write(&target, "/tmp/evil").unwrap();
        std::os::unix::fs::symlink(&target, &file).unwrap();
        assert_eq!(remembered(&file), None);
        fs::remove_dir_all(&directory).unwrap();
    }

    fn good(value: &str) -> LaunchPath {
        LaunchPath {
            value: value.into(),
            issue: None,
        }
    }

    #[test]
    fn a_failed_shell_keeps_the_remembered_path_and_reports_the_failure() {
        use super::super::LaunchPathIssue;
        let failed = LaunchPath {
            value: "/engine/own".into(),
            issue: Some(LaunchPathIssue::ProbeTimedOut),
        };
        let cache = Cache::new();
        let kept = cache.answer(failed.clone(), Some("/remembered".into()));
        assert_eq!(kept.value, "/remembered");
        assert_eq!(kept.issue, Some(LaunchPathIssue::ProbeTimedOut));
        assert_eq!(Cache::new().answer(failed.clone(), None), failed);
    }

    static GATE: AtomicBool = AtomicBool::new(false);
    static ASKED: AtomicUsize = AtomicUsize::new(0);

    fn slow_shell() -> LaunchPath {
        ASKED.fetch_add(1, Ordering::SeqCst);
        while !GATE.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
        good("/fresh:/usr/bin")
    }

    /// The case this module exists for: a restart finds the last answer at once while the
    /// shell, which is slow, is asked once in the background; the first start has nothing
    /// to use and waits for that one question.
    #[test]
    fn a_restart_uses_the_remembered_path_while_the_shell_is_asked_once() {
        let directory = directory();
        // The first start: nothing remembered, so callers wait for the single question.
        let first: &'static Cache = Box::leak(Box::new(Cache::new()));
        first.warm(&directory, slow_shell);
        let waiter = thread::spawn(move || first.resolution(slow_shell));
        thread::sleep(Duration::from_millis(50));
        assert!(!waiter.is_finished(), "no answer yet");
        GATE.store(true, Ordering::SeqCst);
        assert_eq!(waiter.join().unwrap(), good("/fresh:/usr/bin"));
        assert_eq!(
            ASKED.load(Ordering::SeqCst),
            1,
            "one question for all callers"
        );
        // The answer is stored after the waiters are woken.
        while remembered(&directory.join(FILE)).is_none() {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            remembered(&directory.join(FILE)).as_deref(),
            Some("/fresh:/usr/bin")
        );
        // The restart: the shell is slow again, yet the answer is there before it replies.
        GATE.store(false, Ordering::SeqCst);
        let second: &'static Cache = Box::leak(Box::new(Cache::new()));
        second.warm(&directory, slow_shell);
        assert_eq!(second.resolution(slow_shell), good("/fresh:/usr/bin"));
        GATE.store(true, Ordering::SeqCst);
        fs::remove_dir_all(&directory).unwrap();
    }

    static REFRESH_GATE: AtomicBool = AtomicBool::new(false);

    fn refreshed_shell() -> LaunchPath {
        while !REFRESH_GATE.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
        good("/refreshed:/usr/bin")
    }

    /// A rescan asks the shell again, which takes seconds. Whoever holds the Core lock reads
    /// the path meanwhile, and must get the previous answer at once instead of waiting.
    #[test]
    fn a_refresh_leaves_readers_the_previous_path_until_the_shell_answers() {
        let cache: &'static Cache = Box::leak(Box::new(Cache::new()));
        cache.answer(good("/previous:/usr/bin"), None);
        let refreshing = thread::spawn(move || cache.refresh(refreshed_shell));
        thread::sleep(Duration::from_millis(50));
        assert!(!refreshing.is_finished(), "the shell has not answered");
        let started = std::time::Instant::now();
        assert_eq!(
            cache.resolution(refreshed_shell),
            good("/previous:/usr/bin")
        );
        // A reader that waited would block until the gate opens below; the bound is loose so a
        // busy host does not fail it.
        assert!(started.elapsed() < Duration::from_secs(2));
        REFRESH_GATE.store(true, Ordering::SeqCst);
        assert_eq!(refreshing.join().unwrap(), good("/refreshed:/usr/bin"));
        assert_eq!(
            cache.resolution(refreshed_shell),
            good("/refreshed:/usr/bin")
        );
    }
}
