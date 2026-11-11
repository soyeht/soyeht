//! The git that `wt` starts runs none of the programs a repository's configuration names.

use super::{CommandWorktree, LIST_TIMEOUT};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path, process::Command};

fn script(path: &Path, body: &str) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(path)
        .unwrap();
    writeln!(file, "#!/bin/sh\n{body}").unwrap();
}

fn git(repository: &Path, arguments: &[&str]) {
    let mut command = Command::new("git");
    command.current_dir(repository).args(arguments);
    for (name, value) in [("NAME", "t"), ("EMAIL", "t@example.com")] {
        command.env(format!("GIT_AUTHOR_{name}"), value);
        command.env(format!("GIT_COMMITTER_{name}"), value);
    }
    assert!(command.status().unwrap().success(), "{arguments:?}");
}

/// The repository names a file system monitor and a hook; both record that they ran. A `wt`
/// that does what the real one does (it runs git on the repository) must start neither.
#[test]
fn the_git_that_wt_starts_runs_none_of_the_programs_a_repository_names() {
    let directory = std::env::temp_dir().join(format!("sn-wt-hardening-{}", uuid::Uuid::new_v4()));
    let repository = directory.join("repository");
    fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(
        &repository,
        &["commit", "-q", "--allow-empty", "-m", "baseline"],
    );
    let ran = directory.join("ran");
    let program = directory.join("program");
    script(&program, &format!("echo \"$0\" >> '{}'", ran.display()));
    let hook = repository.join(".git/hooks/post-checkout");
    fs::copy(&program, &hook).unwrap();
    git(
        &repository,
        &["config", "core.fsmonitor", &program.to_string_lossy()],
    );
    let wt = directory.join("wt");
    script(
        &wt,
        "git -C \"$2\" status --porcelain >/dev/null\n\
         git -C \"$2\" worktree add -q -b other \"$2/other\"\n\
         echo '{\"schema\":2,\"items\":[]}'",
    );
    let adapter = CommandWorktree::using(wt.to_string_lossy().into());
    let path = crate::agents::environment::launch_search_path();
    let output = adapter
        .wt_on(&path, &repository, &["list"], LIST_TIMEOUT)
        .unwrap();
    let recorded = fs::read_to_string(&ran).unwrap_or_default();
    let added = repository.join("other").is_dir();
    fs::remove_dir_all(&directory).unwrap();
    assert!(
        output.status.success() && added,
        "the stand-in wt did not run git"
    );
    assert!(recorded.is_empty(), "git started: {recorded}");
}
