//! Enforces ADR 0079: the engine never runs an external command except
//! through `soyeht_process::run`, which has a deadline and refuses to start
//! on a thread that holds the Core lock.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Crates whose code runs inside the engine process.
const ENGINE_CRATES: &[&str] = &[
    "soyeht-engine",
    "soyeht-vcs",
    "soyeht-web",
    "soyeht-editor",
    "soyeht-usage",
    "soyeht-writing",
    "soyeht-store",
    "soyeht-lsp",
    "soyeht-skills",
];

/// Files allowed to run a child process directly, and why that is safe.
const DIRECT_PROCESS_FILES: &[(&str, &str)] = &[
    (
        "soyeht-engine/src/agents/environment/probe.rs",
        "probes the login shell once in Core::open, before the Core is shared",
    ),
    (
        "soyeht-engine/src/usage/process.rs",
        "the Codex JSON-RPC conversation writes and reads one long-lived child; curl and \
         security go through soyeht_process::run_with_input. It runs only from \
         PreparedRefresh::read, off the lock, with its own deadline",
    ),
    (
        "soyeht-engine/src/usage/terminal.rs",
        "a usage source driven through a hidden pseudoterminal; it runs only from \
         PreparedRefresh::read, off the lock, with a deadline and a stopped process group",
    ),
];

fn engine_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The source before its first `#[cfg(test)]` module.
fn shipped(source: &str) -> &str {
    let lines = source.lines().collect::<Vec<_>>();
    let mut offset = 0;
    for (index, line) in lines.iter().enumerate() {
        if *line == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.contains("mod "))
        {
            return &source[..offset];
        }
        offset += line.len() + 1;
    }
    source
}

/// A std `Command` run without the bounded runner.
fn runs_process_directly(source: &str) -> bool {
    source.contains("Stdio::")
        || source.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with(".output()")
                || line.starts_with(".status()")
                || line.starts_with(".spawn()")
                || line.contains("Command::new(") && line.contains(").output()")
        })
}

#[test]
fn every_engine_command_runs_through_the_bounded_runner() {
    let root = engine_root();
    let mut violations = Vec::new();
    let mut allowed_seen = Vec::new();
    for krate in ENGINE_CRATES {
        let mut files = Vec::new();
        rust_files(&root.join(krate).join("src"), &mut files);
        for file in files {
            // Forward slashes on every system, as the allowlist spells them.
            let relative = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(&file).unwrap();
            let shipped = shipped(&source);
            if DIRECT_PROCESS_FILES
                .iter()
                .any(|(allowed, _)| *allowed == relative)
            {
                allowed_seen.push(relative);
                continue;
            }
            if runs_process_directly(shipped) {
                violations.push(format!("{relative}: runs a process directly"));
            }
            // `run_git` and `run_git_with` are `run` behind the check that git can start without a
            // system prompt (ADR 0093).
            let bounded = [
                "soyeht_process::run(",
                "soyeht_process::run_git(",
                "soyeht_process::run_git_with(",
            ];
            if shipped.contains("Command::new(")
                && !bounded.iter().any(|call| shipped.contains(call))
            {
                violations.push(format!(
                    "{relative}: builds a Command but never runs it through soyeht_process::run"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "external commands must use soyeht_process::run off the Core lock (ADR 0079):\n{}",
        violations.join("\n")
    );
    allowed_seen.sort();
    let mut allowed = DIRECT_PROCESS_FILES
        .iter()
        .map(|(file, _)| (*file).to_owned())
        .collect::<Vec<_>>();
    allowed.sort();
    assert_eq!(
        allowed_seen, allowed,
        "an allowlisted file moved or was removed"
    );
}

#[test]
fn the_rule_recognises_a_direct_process_run() {
    assert!(runs_process_directly(
        "let output = Command::new(\"git\").output();"
    ));
    assert!(runs_process_directly(
        "Command::new(\"git\")\n        .arg(\"status\")\n        .output()\n"
    ));
    assert!(runs_process_directly(".stdout(Stdio::piped())"));
    assert!(!runs_process_directly(
        "\"terminal/output\" => terminal.output(),"
    ));
    assert_eq!(
        shipped("fn a() {}\n#[cfg(test)]\nmod tests {\n    Stdio::piped();\n}\n"),
        "fn a() {}\n"
    );
}

/// Where the engine waits for the supervisor (`Supervisor::call`, `call_each`, `send_input`), by
/// file, and what runs it. Every one is the work of a job (`Core::spawn_background`,
/// `spawn_prompt_background`, `start_process`, `start_input`) or of an `Outcome::Offload`, never
/// code that holds the Core lock; the apply of each settles the answer under the lock. The table
/// may not grow: a new wait is a new job, and its file and count are added here with the reason
/// (ADR 0117, ADR 0126). A wait on the caller's thread under the lock fails here.
const SUPERVISOR_WAITS: &[(&str, usize, &str)] = &[
    (
        "soyeht-engine/src/agents/recovery.rs",
        2,
        "job: the requests a gap hides",
    ),
    (
        "soyeht-engine/src/agents/switching.rs",
        2,
        "jobs: a switch step, the retired closes",
    ),
    (
        "soyeht-engine/src/agents/switching/candidate.rs",
        2,
        "work of the switch step jobs",
    ),
    (
        "soyeht-engine/src/core.rs",
        2,
        "the connect job and Core::open, before the Core is shared",
    ),
    ("soyeht-engine/src/input.rs", 1, "offload: pane.input"),
    (
        "soyeht-engine/src/process_start.rs",
        1,
        "launch: job or offload",
    ),
    ("soyeht-engine/src/spawn_retry.rs", 3, "jobs: closes"),
    (
        "soyeht-engine/src/supervisor_handle.rs",
        5,
        "the handle itself, start_input, the follow-up job",
    ),
    (
        "soyeht-engine/src/terminal_drain.rs",
        3,
        "jobs: the drain, the exit lookup and the ack",
    ),
];

/// The waits in `shipped` source: `Supervisor::call`, `call_each` and `send_input`.
fn supervisor_waits(shipped: &str) -> usize {
    [".call", ".call_each", ".send_input", "pty_request"]
        .iter()
        .map(|name| calls_to(shipped, name))
        .sum()
}

/// The files that may name `call_supervisor`, the call that waits for the supervisor on a port
/// it is handed: the port's own module and the handle that wraps it.
const CALL_SUPERVISOR_FILES: &[&str] = &[
    "soyeht-engine/src/ports.rs",
    "soyeht-engine/src/supervisor_handle.rs",
];

/// The calls in `shipped` source to `name(`, outside comments and the definition of `name`.
fn calls_to(shipped: &str, name: &str) -> usize {
    let call = format!("{name}(");
    let definition = format!("fn {name}(");
    shipped
        .lines()
        .filter(|line| !line.trim_start().starts_with("//") && !line.contains(&definition))
        .map(|line| line.matches(&call).count())
        .sum()
}

#[test]
fn requests_to_the_supervisor_under_the_core_lock_do_not_grow() {
    let root = engine_root();
    let mut files = Vec::new();
    rust_files(&root.join("soyeht-engine").join("src"), &mut files);
    let mut found = Vec::new();
    let mut stray = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // A file that is only test code (`tests.rs`, `test_panes.rs`, `save_temporary_tests.rs`)
        // is not shipped; a production file with "test" inside its name still is.
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if name == "tests.rs"
            || name.starts_with("test_")
            || name.ends_with("_tests.rs")
            || relative.contains("/tests/")
        {
            continue;
        }
        let source = fs::read_to_string(&file).unwrap();
        let shipped = shipped(&source);
        let waiting = supervisor_waits(shipped);
        if waiting > 0 {
            found.push((relative.clone(), waiting));
        }
        if calls_to(shipped, "call_supervisor") > 0
            && !CALL_SUPERVISOR_FILES.contains(&relative.as_str())
        {
            stray.push(relative);
        }
    }
    found.sort();
    assert!(
        stray.is_empty(),
        "call_supervisor waits for the supervisor on the caller's thread; use Supervisor::call \
         off the Core lock (ADR 0117):\n{}",
        stray.join("\n")
    );
    let allowed = SUPERVISOR_WAITS
        .iter()
        .map(|(file, count, _)| ((*file).to_owned(), *count))
        .collect::<Vec<_>>();
    assert_eq!(
        found, allowed,
        "a request to the supervisor waits somewhere that is not a known job or offload (ADR 0117, \
         ADR 0126): wait through Supervisor::call off the lock, in an Outcome::Offload or a \
         Core::spawn_background job, settle the answer with Core::settle_supervisor, and add the \
         site to SUPERVISOR_WAITS with its reason"
    );
}

#[test]
fn the_supervisor_rule_recognises_a_waiting_call() {
    assert_eq!(
        calls_to("self.pty_request(Request::List)?;", "pty_request"),
        1
    );
    assert_eq!(
        calls_to("pub fn pty_request(&mut self) {}", "pty_request"),
        0
    );
    assert_eq!(calls_to("// self.pty_request(a)", "pty_request"), 0);
    assert_eq!(
        calls_to(
            "self.pty_request(a)?;\nself.pty_request(b)?;",
            "pty_request"
        ),
        2
    );
}

#[test]
fn the_supervisor_wait_rule_counts_every_way_to_wait() {
    assert_eq!(
        supervisor_waits("let r = supervisor.call(Request::List)?;\nlink.call_each(batch);"),
        2
    );
    assert_eq!(
        supervisor_waits("link.send_input(&turn, &carried, request)"),
        1
    );
    assert_eq!(supervisor_waits("self.pty_request(Request::List)?;"), 1);
    // Definitions and comments are not waits.
    assert_eq!(
        supervisor_waits("pub fn call(&self) {}\n// supervisor.call(a)\nfn send_input(&self) {}"),
        0
    );
}
