//! A stand-in for the `wt` worktree command, driven by the engine integration tests. It keeps
//! worktrees as folders under `<repository>/.test-worktrees` and answers with the same JSON the
//! real command prints. Invoked as `wt <flag> <repository> <command> ... <branch>`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const BRANCH_FILE: &str = ".soyeht-test-branch";

fn list(repository: &Path) {
    let mut paths: Vec<PathBuf> = fs::read_dir(repository.join(".test-worktrees"))
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();
    let items: Vec<_> = paths
        .iter()
        .map(|path| {
            let mut item = json!({
                "branch": fs::read_to_string(path.join(BRANCH_FILE)).unwrap_or_default(),
                "head": {"sha": "abc"},
                "worktree": {
                    "path": path,
                    "changes": {
                        "staged": false,
                        "modified": path.join(".dirty").exists(),
                        "untracked": false,
                        "renamed": false,
                        "deleted": false,
                    },
                },
            });
            if !path.join(".unreachable").exists() {
                item["default_branch"] = json!({"integration": {"reason": "ancestor"}});
            }
            item
        })
        .collect();
    println!("{}", json!({"schema": 2, "items": items}));
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let (Some(repository), Some(command)) = (arguments.get(1), arguments.get(2)) else {
        return ExitCode::from(2);
    };
    let repository = Path::new(repository);
    if command == "list" {
        list(repository);
        return ExitCode::SUCCESS;
    }
    let branch = arguments.last().cloned().unwrap_or_default();
    let path = repository
        .join(".test-worktrees")
        .join(branch.replace('/', "-"));
    match command.as_str() {
        "switch" => {
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join(BRANCH_FILE), &branch).unwrap();
            println!("{}", json!({"path": path, "branch": branch}));
        }
        "remove" => {
            let _ = fs::remove_dir_all(&path);
            println!(
                "{}",
                json!({"kind": "worktree", "branch": branch, "path": path,
                    "branch_outcome": "not_attempted"})
            );
        }
        _ => return ExitCode::from(2),
    }
    ExitCode::SUCCESS
}
