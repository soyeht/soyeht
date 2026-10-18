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
