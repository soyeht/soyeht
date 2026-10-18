//! The launch `PATH`, resolved once and shared by every launch of this engine.
//!
//! Asking the person's interactive login shell takes seconds on a busy machine (it reads the
//! person's startup files), and the engine cannot listen, nor restore its agents, until it
//! knows the answer. So the engine remembers the last answer in its data directory, uses it
//! at once on the next start and asks the shell again in the background.
//! Only a caller that finds no answer at all waits for the shell.

use super::LaunchPath;
use crate::streak::Streak;
use soyeht_platform::{
    fs::OpenOptionsPrivateExt,
    log::{Failure, Line},
};
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
    /// Counts the answers stored, so an [`Asking`] knows whether its question was answered.
    answers: u64,
    /// The failures of the shell, logged at the first and at each change of kind.
    failures: Streak,
    file: Option<PathBuf>,
}

impl State {
    /// The line to log for the shell's latest outcome, if any. A failure logs its kind only:
    /// the text of an error can carry a path.
    fn report(&mut self, outcome: Option<&dyn Failure>) -> Option<Line> {
        match outcome {
            Some(failure) => {
                let kind = failure.failure_kind();
                self.failures
                    .fail(&kind)
                    .then(|| Line::new("engine.launch_path_probe_failed").field("kind", kind))
            }
            None => self.failures.succeed().map(|attempts| {
                Line::new("engine.launch_path_recovered").field("attempts", attempts)
            }),
        }
    }
}

/// Held by the thread that asks the shell. If that thread unwinds before storing an answer,
/// the question is closed: waiters wake, the previous answer stays, and the next caller
/// asks again. Without it `probing` would stay set and every later caller would wait.
struct Asking<'a> {
    cache: &'a Cache,
    answers: u64,
}

impl Drop for Asking<'_> {
    fn drop(&mut self) {
        let line = {
            let mut state = self.cache.state();
            if !state.probing || state.answers != self.answers {
                return;
            }
            state.probing = false;
            self.cache.answered.notify_all();
            state.report(Some(&ProbePanicked))
        };
        if let Some(line) = line {
            line.emit();
        }
    }
}

/// The shell probe stopped without an answer.
struct ProbePanicked;

impl Failure for ProbePanicked {
    fn failure_kind(&self) -> String {
        "probe_panicked".into()
    }
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
                answers: 0,
                failures: Streak::once(),
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
        let asking = Asking {
            cache: self,
            answers: state.answers,
        };
        drop(state);
        self.ask(asking, probe, None)
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
        let asking = Asking {
            cache: self,
            answers: state.answers,
        };
        drop(state);
        self.ask(asking, probe, previous)
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
        let answers = state.answers;
        state.resolved = remembered
            .clone()
            .map(|value| LaunchPath { value, issue: None });
        let spawned = thread::Builder::new()
            .name("launch-path".into())
            .spawn(move || {
                self.ask(
                    Asking {
                        cache: self,
                        answers,
                    },
                    probe,
                    remembered,
                );
            });
        if spawned.is_err() {
            // The first caller asks the shell itself.
            state.probing = false;
        }
    }

    /// Asks the shell and stores its answer. `asking` closes the question if `probe` panics.
    fn ask(&self, asking: Asking<'_>, probe: Probe, remembered: Option<String>) -> LaunchPath {
        let path = self.answer(probe(), remembered);
        drop(asking);
        path
    }

    /// Stores what the shell said, wakes the waiters and remembers a good answer for the next
    /// start.
    ///
    /// A shell that failed keeps the `PATH` remembered from the last start, with the failure
    /// reported beside it.
    fn answer(&self, probed: LaunchPath, remembered: Option<String>) -> LaunchPath {
        let (path, file, line) = {
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
            state.answers += 1;
            let line = state.report(path.issue.as_ref().map(|issue| issue as &dyn Failure));
            self.answered.notify_all();
            (path, state.file.clone(), line)
        };
        if let Some(line) = line {
            line.emit();
        }
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
    use super::super::LaunchPathIssue;
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

    #[test]
    fn a_failing_shell_is_logged_at_the_first_failure_and_each_change_of_kind() {
        let mut state = Cache::new().state.into_inner().unwrap();
        let failed = |state: &mut State, issue| state.report(Some(&issue as &dyn Failure));
        let json = |line: Option<Line>| line.map(|line| line.json());
        assert_eq!(
            json(failed(&mut state, LaunchPathIssue::ProbeFailed)),
            Some(r#"{"event":"engine.launch_path_probe_failed","kind":"probe_failed"}"#.into())
        );
        assert_eq!(json(failed(&mut state, LaunchPathIssue::ProbeFailed)), None);
        assert_eq!(
            json(failed(&mut state, LaunchPathIssue::ProbeTimedOut)),
            Some(r#"{"event":"engine.launch_path_probe_failed","kind":"probe_timed_out"}"#.into())
        );
        assert_eq!(
            json(state.report(None)),
            Some(r#"{"event":"engine.launch_path_recovered","attempts":"3"}"#.into())
        );
        assert_eq!(
            json(state.report(None)),
            None,
            "a healthy shell logs nothing"
        );
    }

    fn panicking_shell() -> LaunchPath {
        panic!("the shell probe panics");
    }

    fn quick_shell() -> LaunchPath {
        good("/after:/usr/bin")
    }

    /// Runs `work` on a thread and fails the test if it blocks instead of returning.
    fn finishes<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(work());
        });
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the call blocked behind a probe that is gone")
    }

    #[test]
    fn a_probe_that_panics_does_not_leave_the_cache_waiting_for_it() {
        let cache: &'static Cache = Box::leak(Box::new(Cache::new()));
        cache.answer(good("/previous:/usr/bin"), None);
        assert!(
            thread::spawn(move || cache.refresh(panicking_shell))
                .join()
                .is_err()
        );
        assert!(!cache.state().probing);
        // The previous answer is still the answer, and the next refresh asks again.
        assert_eq!(
            finishes(move || cache.resolution(panicking_shell)),
            good("/previous:/usr/bin")
        );
        assert_eq!(
            finishes(move || cache.refresh(quick_shell)),
            good("/after:/usr/bin")
        );
    }

    #[test]
    fn a_first_probe_that_panics_lets_the_next_caller_ask_again() {
        let cache: &'static Cache = Box::leak(Box::new(Cache::new()));
        assert!(
            thread::spawn(move || cache.resolution(panicking_shell))
                .join()
                .is_err()
        );
        assert_eq!(
            finishes(move || cache.resolution(quick_shell)),
            good("/after:/usr/bin")
        );
    }
}
