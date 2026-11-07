//! A repository can come from anyone, and its configuration names programs. Reading it for the
//! editor's gutter (hunks and blame) must start none of them, and a file whose name looks like
//! an option must stay a file name.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_vcs::{Service, SystemGit};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("soyeht-vcs-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let repository = Self {
            root: soyeht_platform::fs::canonicalize(&root).unwrap(),
        };
        repository.git(&["init", "-q"]);
        repository
    }

    fn git(&self, arguments: &[&str]) {
        let identity = [
            ("GIT_AUTHOR_NAME", "attack"),
            ("GIT_AUTHOR_EMAIL", "attack@example.com"),
            ("GIT_COMMITTER_NAME", "attack"),
            ("GIT_COMMITTER_EMAIL", "attack@example.com"),
        ];
        let mut command = Command::new("git");
        command
            .current_dir(&self.root)
            .envs(identity)
            .args(arguments);
        let status = command.output().unwrap().status;
        assert!(status.success(), "{arguments:?}");
    }

    /// A program that records that it ran, as a file named `name` in the repository's folder.
    fn canary(&self, name: &str) -> (String, PathBuf) {
        let marker = self.root.join(format!("ran-{name}"));
        let program = self.root.join(format!("{name}.sh"));
        fs::write(
            &program,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        (program.display().to_string(), marker)
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn reading_a_hostile_repository_starts_no_program_its_configuration_names() {
    let repository = Repository::new("hostile");
    fs::write(
        repository.root.join(".gitattributes"),
        "*.bin diff=probe filter=probe\n*.txt filter=probe diff=probe\n",
    )
    .unwrap();
    fs::write(repository.root.join("a.bin"), "one\ntwo\n").unwrap();
    fs::write(repository.root.join("a.txt"), "one\ntwo\n").unwrap();
    repository.git(&["add", "."]);
    repository.git(&["commit", "-q", "-m", "baseline"]);

    let mut canaries = Vec::new();
    for (key, name) in [
        ("core.fsmonitor", "fsmonitor"),
        ("filter.probe.clean", "clean"),
        ("filter.probe.smudge", "smudge"),
        ("filter.probe.process", "process"),
        ("diff.probe.textconv", "textconv"),
        ("diff.external", "external"),
        ("core.pager", "pager"),
        ("core.editor", "editor"),
        ("core.sshCommand", "ssh"),
        ("core.gitProxy", "proxy"),
        ("credential.helper", "credential"),
        ("gpg.program", "gpg"),
        ("core.askPass", "askpass"),
        ("core.untrackedCache", "unused"),
    ] {
        let (program, marker) = repository.canary(name);
        repository.git(&["config", key, &program]);
        canaries.push((key, marker));
    }
    // Hooks run on writes only, but a hostile folder sets them for every event.
    let hooks = repository.root.join("hooks");
    fs::create_dir_all(&hooks).unwrap();
    let (hook, hook_marker) = repository.canary("hook");
    for event in [
        "post-checkout",
        "post-merge",
        "pre-commit",
        "reference-transaction",
    ] {
        fs::copy(&hook, hooks.join(event)).unwrap();
        fs::set_permissions(hooks.join(event), fs::Permissions::from_mode(0o755)).unwrap();
    }
    repository.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    canaries.push(("core.hooksPath", hook_marker));

    fs::write(repository.root.join("a.bin"), "one\nchanged\n").unwrap();
    // A control: plain git does start these programs here, so the fixture is live and the
    // absence of a marker below means something.
    let _ = Command::new("git")
        .arg("-C")
        .arg(&repository.root)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let _ = Command::new("git")
        .arg("-C")
        .arg(&repository.root)
        .args(["diff", "--ext-diff", "--textconv"])
        .output()
        .unwrap();
    let live = canaries
        .iter()
        .filter(|(_, marker)| marker.exists())
        .count();
    assert!(
        live > 0,
        "no program of the hostile configuration ran under plain git"
    );
    for (_, marker) in &canaries {
        let _ = fs::remove_file(marker);
    }
    let service = Service::new(Box::new(SystemGit));
    for file in ["a.bin", "a.txt", "missing.txt", "sub/../a.bin"] {
        let _ = service.inspect(&repository.root, Path::new(file), "one\nchanged\n");
    }
    // Opened from a folder inside the repository too.
    fs::create_dir_all(repository.root.join("nested")).unwrap();
    let _ = service.inspect(
        &repository.root.join("nested"),
        Path::new("../a.bin"),
        "text",
    );

    for (key, marker) in canaries {
        assert!(
            !marker.exists(),
            "{key} started a program of the repository"
        );
    }
}

#[test]
fn a_file_named_like_an_option_is_a_file_and_never_an_option() {
    let repository = Repository::new("option-name");
    let output = repository.root.join("injected");
    let names = [
        "--output=injected".to_owned(),
        "-p".to_owned(),
        "--ext-diff".to_owned(),
        "--no-index".to_owned(),
        "-c".to_owned(),
    ];
    for name in &names {
        fs::write(repository.root.join(name), "one\ntwo\n").unwrap();
    }
    repository.git(&["add", "--", "."]);
    repository.git(&["commit", "-q", "-m", "baseline", "--no-verify"]);

    let service = Service::new(Box::new(SystemGit));
    for name in &names {
        let found = service.inspect(&repository.root, Path::new(name), "one\ntwo\nthree\n");
        assert!(found.is_ok(), "{name}: {found:?}");
    }
    assert!(!output.exists(), "a file name was taken as an option");
}
