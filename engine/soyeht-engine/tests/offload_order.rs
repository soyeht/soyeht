//! A slow read that runs off the Core lock must not hold back the requests behind it on the
//! same connection (ADR 0079, ADR 0095). The editor asks for `file.diff` after every
//! `file.open`; `git` is the slow part, and a person who clicks the next file meanwhile waits
//! for text, not for the diff of the file before.
//! The fixture is a POSIX shell `git` that sleeps, so the check runs where there is a shell.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use support::Rig;

/// The pause before every `git` the engine starts. A diff starts several, so it takes a few
/// times this long.
const GIT_PAUSE: &str = "0.8";
/// What the engine takes to open a small file, with a wide margin for a loaded host. The
/// diff behind it takes more than three seconds, so a reply faster than this was not queued.
const PROMPT: Duration = Duration::from_millis(1_500);

fn git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args([
            "-c",
            "user.name=Writer",
            "-c",
            "user.email=w@example.invalid",
        ])
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Puts a `git` that waits `pause` seconds ahead of the real one on the engine's path.
fn install_slow_git(rig: &Rig, pause: &str) {
    let search = std::env::var("PATH").unwrap();
    let shim = rig.path.join("bin/git");
    fs::create_dir_all(shim.parent().unwrap()).unwrap();
    fs::write(
        &shim,
        format!("#!/bin/sh\nPATH='{search}'\nsleep {pause}\nexec git \"$@\"\n"),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_slow_file_diff_does_not_hold_back_the_next_file_on_its_connection() {
    let rig = Rig::new();
    let repository = rig.path.join("order-repository");
    fs::create_dir_all(&repository).unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(repository.join(name), format!("{name}\nline two\n")).unwrap();
    }
    git(&repository, &["init", "-q"]);
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-qm", "baseline"]);
    install_slow_git(&rig, GIT_PAUSE);

    let mut client = rig.client();
    let workspace = client.add_workspace(&repository, "order-workspace");

    let started = Instant::now();
    let first = client.open_file(&workspace, "a.txt");
    let first_open = started.elapsed();
    assert_eq!(first["viewport"]["lines"][0]["text"], "a.txt");
    let file_id = first["file"]["fileId"].as_str().unwrap();
    let revision = first["file"]["revision"].as_str().unwrap();

    let diff_started = Instant::now();
    let diff = client.send("file.diff", json!({"fileId":file_id,"revision":revision}));
    let clicked = Instant::now();
    let second = client.open_file(&workspace, "b.txt");
    let second_open = clicked.elapsed();
    assert_eq!(second["viewport"]["lines"][0]["text"], "b.txt");
    let read = client.call(
        "file.read",
        json!({"fileId":second["file"]["fileId"],"fromLine":0,"lineCount":10}),
    );
    let second_read = clicked.elapsed();
    assert_eq!(read["lines"][0]["text"], "b.txt");

    let reply = client.wait(diff);
    let diff_took = diff_started.elapsed();
    assert!(reply.get("error").is_none(), "{reply}");
    eprintln!(
        "first file.open {first_open:?}; second file.open {second_open:?}; \
         second file.read done {second_read:?}; file.diff {diff_took:?}"
    );
    assert!(
        diff_took > PROMPT * 2,
        "the fixture must make the diff slow, it took {diff_took:?}"
    );
    assert!(
        second_read < PROMPT,
        "file.open and file.read waited {second_read:?} behind the file.diff of the file before"
    );
}

/// The connection stops reading at the ninth diff until one ends, and every reply still arrives.
#[test]
fn a_connection_reads_on_while_at_most_eight_of_its_diffs_are_in_progress() {
    const CAP: usize = 8;
    let rig = Rig::new();
    let repository = rig.path.join("cap-repository");
    fs::create_dir_all(&repository).unwrap();
    fs::write(repository.join("a.txt"), "a\n").unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-qm", "baseline"]);
    // A diff runs five git commands one after the other, so the first ends after about a second.
    install_slow_git(&rig, "0.2");
    let mut client = rig.client();
    let workspace = client.add_workspace(&repository, "cap-workspace");
    let opened = client.open_file(&workspace, "a.txt");
    let file_id = opened["file"]["fileId"].clone();
    let revision = opened["file"]["revision"].clone();

    let diffs = (0..=CAP)
        .map(|_| client.send("file.diff", json!({"fileId":file_id,"revision":revision})))
        .collect::<Vec<_>>();
    let asked = Instant::now();
    client.call("engine.status", json!({}));
    let waited = asked.elapsed();
    for diff in diffs {
        assert!(client.wait(diff).get("error").is_none());
    }

    assert!(
        waited > Duration::from_millis(800),
        "a request behind {} diffs was read after {waited:?}",
        CAP + 1
    );
}
