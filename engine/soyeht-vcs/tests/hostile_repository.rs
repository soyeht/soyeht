//! A repository can come from anyone, and its configuration names programs. Reading it for the
//! editor's gutter (hunks and blame) must start none of them, and a file whose name looks like
//! an option must stay a file name.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::testing::scratch_dir;
use soyeht_vcs::{Error, Service, SystemGit};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new(label: &str) -> Self {
        let root = scratch_dir(label);
        let repository = Self {
            root: soyeht_platform::fs::canonicalize(&root).unwrap(),
        };
        repository.git(&["init", "-q"]);
        repository
    }

    /// Git as a person runs it in the repository: no terminal, no editor or program chosen by
    /// the environment (those would win over the repository's configuration), and a
    /// terminal type that lets `core.editor` start.
    fn plain_git(&self, arguments: &[&str], input: &str) -> Output {
        let mut command = Command::new("git");
        command
            .current_dir(&self.root)
            .env("GIT_AUTHOR_NAME", "attack")
            .env("GIT_AUTHOR_EMAIL", "attack@example.com")
            .env("GIT_COMMITTER_NAME", "attack")
            .env("GIT_COMMITTER_EMAIL", "attack@example.com")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("TERM", "xterm")
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for variable in [
            "GIT_EDITOR",
            "EDITOR",
            "VISUAL",
            "GIT_PAGER",
            "GIT_ASKPASS",
            "SSH_ASKPASS",
            "GIT_SSH",
            "GIT_SSH_COMMAND",
            "GIT_PROXY_COMMAND",
            "GIT_EXTERNAL_DIFF",
        ] {
            command.env_remove(variable);
        }
        let mut child = command.spawn().unwrap();
        // A command that does not read its input closes the pipe; that is not a failure here.
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        child.wait_with_output().unwrap()
    }

    fn git(&self, arguments: &[&str]) {
        let output = self.plain_git(arguments, "");
        assert!(output.status.success(), "{arguments:?}");
    }

    /// A program that records that it ran, as a file named `ran-<name>` in the repository's
    /// folder. It reads nothing, so git never waits for it.
    fn canary(&self, name: &str) -> (String, PathBuf) {
        let marker = self.root.join(format!("ran-{name}"));
        let program = self.root.join(format!("{name}.sh"));
        fs::write(
            &program,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
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

/// A setting of a repository's configuration that names a program, and the plain git commands
/// (with the text they read) that start it. Each case runs in a repository of its own, so one
/// setting cannot hide another (a `process` filter replaces `clean` and `smudge`, an external
/// diff replaces `textconv`).
struct Setting {
    key: &'static str,
    controls: &'static [(&'static [&'static str], &'static str)],
}

/// Every setting that names a program and that plain git starts from a command that needs no
/// terminal and no network. Left out, because plain git does not run them here: `core.pager`
/// (git starts a pager only when its output is a terminal, and every command of the crate writes
/// to a pipe) and `core.untrackedCache` (a switch, it names no program).
const SETTINGS: [Setting; 11] = [
    Setting {
        key: "core.fsmonitor",
        controls: &[(&["status", "--porcelain"], "")],
    },
    Setting {
        key: "filter.probe.clean",
        controls: &[(&["add", "-A"], "")],
    },
    Setting {
        key: "filter.probe.smudge",
        controls: &[(&["checkout", "--", "a.txt"], "")],
    },
    Setting {
        key: "filter.probe.process",
        controls: &[(&["add", "-A"], "")],
    },
    Setting {
        key: "diff.probe.textconv",
        controls: &[(&["diff"], "")],
    },
    Setting {
        key: "diff.external",
        controls: &[(&["diff"], "")],
    },
    Setting {
        key: "core.editor",
        controls: &[(&["commit", "--allow-empty"], "")],
    },
    Setting {
        key: "core.sshCommand",
        controls: &[(&["ls-remote", "ssh://localhost/repository"], "")],
    },
    Setting {
        key: "core.gitProxy",
        controls: &[(&["ls-remote", "git://example.invalid/repository"], "")],
    },
    Setting {
        key: "credential.helper",
        controls: &[(
            &["credential", "fill"],
            "protocol=https\nhost=example.com\n\n",
        )],
    },
    Setting {
        key: "gpg.program",
        controls: &[(&["commit", "--allow-empty", "-S", "-m", "signed"], "")],
    },
];

/// The attribute file that sends the repository's files through the filter and the converter.
const ATTRIBUTES: &str = "*.bin diff=probe filter=probe\n*.txt filter=probe diff=probe\n";

/// A repository with a committed baseline, `nested/b.txt` in a folder, a branch one commit
/// ahead (for the merge hook), and a changed working copy.
fn repository_with_baseline(label: &str) -> Repository {
    let repository = Repository::new(label);
    fs::write(repository.root.join(".gitattributes"), ATTRIBUTES).unwrap();
    fs::write(repository.root.join("a.bin"), "one\ntwo\n").unwrap();
    fs::write(repository.root.join("a.txt"), "one\ntwo\n").unwrap();
    fs::create_dir_all(repository.root.join("nested")).unwrap();
    fs::write(repository.root.join("nested/b.txt"), "one\ntwo\n").unwrap();
    repository.git(&["add", "."]);
    repository.git(&["commit", "-q", "-m", "baseline"]);
    repository.git(&["branch", "ahead"]);
    repository.git(&["checkout", "-q", "ahead"]);
    repository.git(&["commit", "-q", "--allow-empty", "-m", "ahead"]);
    repository.git(&["checkout", "-q", "-"]);
    fs::write(repository.root.join("a.bin"), "one\nchanged\n").unwrap();
    fs::write(repository.root.join("a.txt"), "one\nchanged\n").unwrap();
    repository
}

/// The gutter of a changed file, read the way the editor reads it: it must succeed, with hunks
/// and blame, so the commands the crate runs (the root, the baseline and the blame) all ran.
fn inspect_changed(repository: &Repository, workspace: &str, file: &str) {
    let found = Service::new(Box::new(SystemGit))
        .inspect(
            &repository.root.join(workspace),
            Path::new(file),
            "one\nchanged\n",
        )
        .unwrap_or_else(|error| panic!("{workspace}/{file}: {error:?}"));
    assert!(
        !found.hunks.is_empty() && !found.blame.is_empty(),
        "{workspace}/{file}"
    );
}

/// A changed file of each kind, and one opened from a folder inside the repository.
fn inspect_everything(repository: &Repository) {
    inspect_changed(repository, "", "a.bin");
    inspect_changed(repository, "", "a.txt");
    inspect_changed(repository, "nested", "b.txt");
}

/// The two answers that are not a diff against a baseline, and that the tests above leave out
/// because they cost a process each: a path that is not committed yet, and a path that leaves
/// the repository.
#[test]
fn an_uncommitted_path_is_new_and_a_path_that_leaves_the_repository_is_refused() {
    let repository = repository_with_baseline("paths");
    let service = Service::new(Box::new(SystemGit));
    let found = service
        .inspect(&repository.root, Path::new("missing.txt"), "one\n")
        .unwrap();
    assert!(found.blame.is_empty() && !found.hunks.is_empty());
    for escaping in ["sub/../a.bin", "../a.bin"] {
        assert_eq!(
            service
                .inspect(&repository.root, Path::new(escaping), "text")
                .unwrap_err(),
            Error::InvalidPath,
            "{escaping}"
        );
    }
}

#[test]
fn reading_a_hostile_repository_starts_no_program_its_configuration_names() {
    for setting in SETTINGS {
        let repository = repository_with_baseline("hostile");
        let (program, marker) = repository.canary("probe");
        repository.git(&["config", setting.key, &program]);
        // The control: plain git starts the program for this setting, so the fixture is live and
        // the absence of the marker below means something.
        let mut fixture_live = false;
        for (arguments, input) in setting.controls {
            repository.plain_git(arguments, input);
            fixture_live |= marker.exists();
        }
        assert!(
            fixture_live,
            "{}: plain git did not start the program of the hostile configuration",
            setting.key
        );
        fs::remove_file(&marker).unwrap();
        // The control may have changed the repository (an add, a commit); the baseline stays.
        fs::write(repository.root.join("a.bin"), "one\nchanged\n").unwrap();
        fs::write(repository.root.join("a.txt"), "one\nchanged\n").unwrap();

        // One process per setting is enough to start the program, and a process is slow.
        inspect_changed(&repository, "", "a.bin");
        assert!(
            !marker.exists(),
            "{} started a program of the repository",
            setting.key
        );
    }
}

/// The hooks of `core.hooksPath` run on writes only, but a hostile folder sets them for every
/// event. Each has its own program and marker, and its own control.
#[test]
fn reading_a_hostile_repository_runs_none_of_its_hooks() {
    const CONTROLS: [(&str, &[&str]); 4] = [
        // The merge comes first: a commit of the repository's own branch would end the fast-forward.
        ("post-merge", &["merge", "-q", "--ff-only", "ahead"]),
        ("pre-commit", &["commit", "--allow-empty", "-m", "control"]),
        (
            "reference-transaction",
            &["commit", "--allow-empty", "-m", "control"],
        ),
        ("post-checkout", &["checkout", "-q", "-b", "control"]),
    ];
    let repository = repository_with_baseline("hooks");
    let hooks = repository.root.join("hooks");
    fs::create_dir_all(&hooks).unwrap();
    let mut markers = Vec::new();
    for (event, _) in CONTROLS {
        let (program, marker) = repository.canary(event);
        fs::copy(&program, hooks.join(event)).unwrap();
        fs::set_permissions(hooks.join(event), fs::Permissions::from_mode(0o755)).unwrap();
        markers.push((event, marker));
    }
    repository.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    for (_, arguments) in CONTROLS {
        repository.plain_git(arguments, "");
    }
    for (event, marker) in &markers {
        assert!(
            marker.exists(),
            "plain git did not run the {event} hook of the hostile configuration"
        );
        fs::remove_file(marker).unwrap();
    }
    fs::write(repository.root.join("a.bin"), "one\nchanged\n").unwrap();
    fs::write(repository.root.join("a.txt"), "one\nchanged\n").unwrap();

    inspect_everything(&repository);
    for (event, marker) in &markers {
        assert!(!marker.exists(), "reading ran the {event} hook");
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
