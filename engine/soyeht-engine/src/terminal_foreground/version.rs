//! The version of a terminal harness, read once per file and remembered (ADR 0165).
//!
//! `<harness> --version` runs through `soyeht_process::run`, with a deadline, off the Core lock.
//! The answer is kept by the file that was asked and the time it was last changed, so the next
//! start of the same harness reads nothing and an update of the file asks again. A harness that
//! did not answer is not asked again for a while.

use super::harnesses::{Matched, Via};
use soyeht_process::Limits;
use soyeht_ptyd::wire::Program;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, SystemTime},
};

/// How long `--version` may take. A harness that is slow to start is not waited for.
const DEADLINE: Duration = Duration::from_secs(5);
/// The most of `--version`'s output that is read: a version is one short line.
const OUTPUT_BYTES: usize = 1024;
/// The longest a version is, as the contract says (`TerminalHarness.version`).
const LONGEST: usize = 64;
/// How long a harness that did not answer is left alone.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// How many files the cache remembers; the oldest answers go first.
const REMEMBERED: usize = 32;

/// The command that asks a harness its version, and the file whose change makes the answer stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Question {
    program: PathBuf,
    /// What goes before `--version` (the script of a host program).
    leading: Vec<String>,
    /// The file the harness is: its change time says whether an answer is still good.
    file: PathBuf,
}

/// What to run to read the version of `matched`, or `None` when its file is not known by a full
/// path (a relative one names a place this engine cannot tell).
pub(super) fn question(matched: &Matched<'_>) -> Option<Question> {
    let Program {
        executable,
        arguments,
        ..
    } = matched.program;
    let absolute = |path: &str| Path::new(path).is_absolute().then(|| PathBuf::from(path));
    if let Via::Script(script) = matched.via {
        let program = absolute(executable.as_deref()?)?;
        let file = absolute(script)?;
        return Some(Question {
            program,
            leading: vec![script.to_owned()],
            file,
        });
    }
    let launched = arguments.first().map(String::as_str).and_then(absolute);
    let file = match launched {
        Some(launched) if matched.known.version_from_launch => launched,
        _ => absolute(executable.as_deref()?)?,
    };
    Some(Question {
        program: file.clone(),
        leading: Vec::new(),
        file,
    })
}

/// The first version-shaped word of what `--version` printed: `2.1.325 (Claude Code)`,
/// `codex-cli 0.164.0` and `2026.09.02-c22c1a3` all read as their number.
pub(super) fn parse(output: &str) -> Option<String> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    line.split_whitespace()
        .map(|word| word.trim_start_matches(['v', 'V']))
        .find(|word| {
            word.starts_with(|first: char| first.is_ascii_digit())
                && word.contains('.')
                && word.chars().all(|character| {
                    character.is_ascii_alphanumeric() || ".-+_".contains(character)
                })
        })
        .map(|word| word.chars().take(LONGEST).collect())
}

/// What the harness at `question` prints for `--version`, or `None` when it did not answer in
/// time or its answer holds no version.
fn ask(question: &Question) -> Option<String> {
    let mut command = Command::new(&question.program);
    command.args(&question.leading).arg("--version");
    // ignored: a harness that cannot be run, or does not answer in time, has no version to show
    let output = soyeht_process::run(command, Limits::new(DEADLINE, OUTPUT_BYTES)).ok()?;
    if !output.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&output.stdout))
}

type Key = (PathBuf, SystemTime);

struct Answer {
    version: Option<String>,
    at: Instant,
}

/// The versions asked so far.
#[derive(Default)]
pub(super) struct Versions {
    answers: BTreeMap<Key, Answer>,
}

impl Versions {
    /// The answer to give for `key`, if one is still good: a version stays good while the file
    /// is the same, and a failure only for a while.
    fn fresh(&self, key: &Key, now: Instant) -> Option<Option<String>> {
        let answer = self.answers.get(key)?;
        (answer.version.is_some() || now.duration_since(answer.at) < RETRY_AFTER)
            .then(|| answer.version.clone())
    }

    fn remember(&mut self, key: Key, version: Option<String>, now: Instant) {
        self.answers.insert(key, Answer { version, at: now });
        while self.answers.len() > REMEMBERED {
            let Some(oldest) = self
                .answers
                .iter()
                .min_by_key(|(_, answer)| answer.at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.answers.remove(&oldest);
        }
    }
}

/// The version of the harness at `question`, from `cache` or, when the file is new or changed,
/// from `ask`. The cache is locked to look and to write, never while `ask` runs. A file that
/// cannot be seen is not run.
pub(super) fn version_of(
    cache: &Mutex<Versions>,
    question: &Question,
    now: Instant,
    ask: impl FnOnce(&Question) -> Option<String>,
) -> Option<String> {
    let changed = std::fs::metadata(&question.file).and_then(|meta| meta.modified());
    // ignored: a file with no readable change time is not trusted to be the same file later, so
    // it is not run and has no version to show
    let changed = changed.ok()?;
    let key: Key = (question.program.clone(), changed);
    if let Some(known) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .fresh(&key, now)
    {
        return known;
    }
    let version = ask(question);
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remember(key, version.clone(), now);
    version
}

/// [`version_of`] by running the harness.
pub(super) fn read(cache: &Mutex<Versions>, question: &Question) -> Option<String> {
    version_of(cache, question, Instant::now(), ask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_foreground::harnesses::identify;
    use std::cell::Cell;

    fn program(exe: &str, args: &[&str]) -> Program {
        super::super::harnesses::sample(1, None, exe, args)
    }

    #[test]
    fn what_each_harness_prints_reads_as_its_version() {
        for (printed, version) in [
            ("2.1.325 (Claude Code)\n", "2.1.325"),
            ("codex-cli 0.164.0\n", "0.164.0"),
            ("2.2.5\n", "2.2.5"),
            ("2026.09.02-c22c1a3\n", "2026.09.02-c22c1a3"),
            ("\n  v1.2.3-beta.1+build\n", "1.2.3-beta.1+build"),
        ] {
            assert_eq!(parse(printed).as_deref(), Some(version), "{printed:?}");
        }
        for printed in [
            "",
            "\n",
            "update available\n",
            "error: unknown flag --version\n",
        ] {
            assert_eq!(parse(printed), None, "{printed:?}");
        }
        let long = format!("1.{}", "9".repeat(200));
        assert_eq!(parse(&long).unwrap().chars().count(), LONGEST);
    }

    #[test]
    fn a_harness_is_asked_by_the_file_that_is_it() {
        let claude = program("/h/.local/share/claude/versions/2.1.325", &["claude"]);
        let asked = question(&identify(&claude).unwrap()).unwrap();
        assert_eq!(
            asked.program,
            Path::new("/h/.local/share/claude/versions/2.1.325")
        );
        assert!(asked.leading.is_empty());

        // Cursor's file is the node that hosts it: the program as it was started is asked.
        let script = "/h/.local/share/cursor-agent/versions/1/index.js";
        let cursor = program(
            "/h/.local/share/cursor-agent/versions/1/node",
            &["/h/.local/bin/cursor-agent", "--use-system-ca", script],
        );
        let asked = question(&identify(&cursor).unwrap()).unwrap();
        assert_eq!(asked.program, Path::new("/h/.local/bin/cursor-agent"));

        // Started by a bare name, the bundled file is all there is to ask.
        let bare = program(
            "/h/.local/share/cursor-agent/versions/1/node",
            &["agent", script],
        );
        assert_eq!(
            question(&identify(&bare).unwrap()).unwrap().program,
            Path::new("/h/.local/share/cursor-agent/versions/1/node")
        );
    }

    #[test]
    fn a_script_run_by_node_is_asked_through_node() {
        let script = "/n/node_modules/@openai/codex/bin/codex.js";
        let shim = program("/usr/local/bin/node", &["node", script]);
        let asked = question(&identify(&shim).unwrap()).unwrap();
        assert_eq!(asked.program, Path::new("/usr/local/bin/node"));
        assert_eq!(asked.leading, [script]);
        assert_eq!(asked.file, Path::new(script));
    }

    #[test]
    fn a_file_named_by_a_relative_path_is_not_asked() {
        let relative = program("claude", &["claude"]);
        assert!(question(&identify(&relative).unwrap()).is_none());
    }

    fn file_in(directory: &Path, name: &str) -> Question {
        let file = directory.join(name);
        std::fs::write(&file, "x").unwrap();
        Question {
            program: file.clone(),
            leading: Vec::new(),
            file,
        }
    }

    /// A harness file in a folder of its own, with a cache and a count of the times it was asked.
    struct Asking {
        directory: PathBuf,
        question: Question,
        cache: Mutex<Versions>,
        asked: Cell<u32>,
    }

    impl Asking {
        fn new(name: &str) -> Self {
            let directory = crate::supervisor_link::tests::short_directory();
            let question = file_in(&directory, name);
            Self {
                directory,
                question,
                cache: Mutex::new(Versions::default()),
                asked: Cell::new(0),
            }
        }

        /// The version at `now`; the question, when it is put, is answered by `answer(count)`.
        fn at(&self, now: Instant, answer: impl Fn(u32) -> Option<String>) -> Option<String> {
            version_of(&self.cache, &self.question, now, |_: &Question| {
                self.asked.set(self.asked.get() + 1);
                answer(self.asked.get())
            })
        }

        fn finish(self) {
            std::fs::remove_dir_all(self.directory).unwrap();
        }
    }

    #[test]
    fn a_version_is_asked_once_per_file_and_again_when_the_file_changes() {
        let asking = Asking::new("claude");
        let now = Instant::now();
        let numbered = |count| Some(format!("1.0.{count}"));
        assert_eq!(asking.at(now, numbered).as_deref(), Some("1.0.1"));
        assert_eq!(asking.at(now, numbered).as_deref(), Some("1.0.1"));
        assert_eq!(asking.asked.get(), 1);
        // The file is replaced: a new modification time is a new question.
        std::fs::File::options()
            .write(true)
            .open(&asking.question.file)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        assert_eq!(asking.at(now, numbered).as_deref(), Some("1.0.2"));
        asking.finish();
    }

    #[test]
    fn a_harness_that_did_not_answer_is_left_alone_for_a_while() {
        let asking = Asking::new("opencode");
        let start = Instant::now();
        assert_eq!(asking.at(start, |_| None), None);
        assert_eq!(asking.at(start + Duration::from_secs(30), |_| None), None);
        assert_eq!(asking.asked.get(), 1);
        assert_eq!(asking.at(start + RETRY_AFTER, |_| None), None);
        assert_eq!(asking.asked.get(), 2);
        asking.finish();
    }

    #[test]
    fn a_file_that_cannot_be_seen_is_not_run() {
        let cache = Mutex::new(Versions::default());
        let missing = Question {
            program: PathBuf::from("/nowhere/claude"),
            leading: Vec::new(),
            file: PathBuf::from("/nowhere/claude"),
        };
        let version = version_of(&cache, &missing, Instant::now(), |_| {
            Some("must not be asked".into())
        });
        assert_eq!(version, None);
    }

    #[test]
    fn the_cache_forgets_the_oldest_answers() {
        let mut versions = Versions::default();
        let start = Instant::now();
        let modified = SystemTime::UNIX_EPOCH;
        for number in 0..=REMEMBERED {
            versions.remember(
                (PathBuf::from(format!("/h/{number}")), modified),
                Some("1.0".into()),
                start + Duration::from_secs(number as u64),
            );
        }
        assert_eq!(versions.answers.len(), REMEMBERED);
        assert!(
            !versions
                .answers
                .contains_key(&(PathBuf::from("/h/0"), modified))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_real_program_is_asked_for_its_version() {
        use std::os::unix::fs::PermissionsExt;
        let directory = crate::supervisor_link::tests::short_directory();
        let script = directory.join("claude");
        std::fs::write(&script, "#!/bin/sh\necho \"9.8.7 (fake harness)\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let question = Question {
            program: script.clone(),
            leading: Vec::new(),
            file: script,
        };
        let cache = Mutex::new(Versions::default());
        assert_eq!(read(&cache, &question).as_deref(), Some("9.8.7"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
