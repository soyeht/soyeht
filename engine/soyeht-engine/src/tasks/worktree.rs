//! Worktree process boundary and the first prompt of a dispatch.

use crate::errors::{self, Result};
use serde::Deserialize;
use serde_json::Value;
use soyeht_process::{Limits, Output};
use soyeht_protocol::{Comment, Subitem, Task, TaskExecutor};
use soyeht_store::tasks::DispatchCheckpoint;
use std::{path::Path, process::Command, time::Duration};

const OUTPUT_LIMIT: usize = 1_048_576;
/// `wt list` reads every worktree's status.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// `wt switch` checks out a whole tree; `wt remove` deletes one.
const CHANGE_TIMEOUT: Duration = Duration::from_secs(120);
/// `lsof +D` walks the worktree.
#[cfg(unix)]
const PROCESS_SCAN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedWorktree {
    pub path: String,
    pub branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanOutcome {
    pub cleaned: bool,
    pub reason: Option<String>,
}

/// Runs `wt`, git and `lsof`. The engine calls it only off the Core lock
/// (ADR 0079).
pub trait WorktreePort: Send {
    fn prepare(&mut self, repository: &Path, branch: &str) -> Result<PreparedWorktree>;
    fn clean(&mut self, repository: &Path, worktree: &Path, branch: &str) -> Result<CleanOutcome>;
}

pub struct CommandWorktree {
    executable: String,
}

impl Default for CommandWorktree {
    fn default() -> Self {
        Self {
            executable: "wt".into(),
        }
    }
}

impl CommandWorktree {
    #[cfg(all(test, unix))]
    fn using(executable: String) -> Self {
        Self { executable }
    }

    fn run(&self, repository: &Path, args: &[&str]) -> Result<Output> {
        validate_target(repository, args.last().copied().unwrap_or_default())?;
        self.wt(repository, args, CHANGE_TIMEOUT)
    }

    fn wt(&self, repository: &Path, args: &[&str], timeout: Duration) -> Result<Output> {
        // `wt` starts git itself, so the check comes before `wt` does.
        soyeht_process::require_git().map_err(errors::worktree_process)?;
        let mut command = Command::new(&self.executable);
        command.arg("-C").arg(repository).args(args);
        bounded_command(command, timeout)
    }

    fn list(&self, repository: &Path, branch: &str) -> Result<(Value, BranchLocation)> {
        validate_target(repository, branch)?;
        let output = self.wt(
            repository,
            &["list", "--format=json", "--branches"],
            LIST_TIMEOUT,
        )?;
        let value = checked_json(output)?;
        let location = find_branch(&value, branch).unwrap_or(BranchLocation::Missing);
        Ok((value, location))
    }
}

impl WorktreePort for CommandWorktree {
    fn prepare(&mut self, repository: &Path, branch: &str) -> Result<PreparedWorktree> {
        let (_, location) = self.list(repository, branch)?;
        if let BranchLocation::Worktree(path) = location {
            return checked_prepared(path, branch);
        }
        let mut args = vec!["switch", "--no-cd", "--no-hooks", "--format=json"];
        if location == BranchLocation::Missing {
            args.push("--create");
        }
        args.push(branch);
        let value = checked_json(self.run(repository, &args)?)?;
        let answer: SwitchAnswer = serde_json::from_value(value)
            .map_err(|_| errors::worktree_unavailable("Invalid worktree response"))?;
        if answer.branch.as_deref() != Some(branch) {
            return Err(errors::worktree_unavailable(
                "Worktree response changed the requested branch",
            ));
        }
        checked_prepared(answer.path, branch)
    }

    fn clean(&mut self, repository: &Path, worktree: &Path, branch: &str) -> Result<CleanOutcome> {
        let (listing, location) = self.list(repository, branch)?;
        match location {
            BranchLocation::BranchOnly => {
                return Ok(CleanOutcome {
                    cleaned: true,
                    reason: None,
                });
            }
            BranchLocation::Missing => {
                return refused("Task branch is not registered");
            }
            BranchLocation::Worktree(path) if Path::new(&path) != worktree => {
                return refused("Task branch is checked out in another worktree");
            }
            BranchLocation::Worktree(_) => {}
        }
        if let Some(reason) = cleanup_refusal(&listing, branch)? {
            return refused(reason);
        }
        if process_uses(worktree)? {
            return refused("A process is using the worktree");
        }
        let output = self.run(
            repository,
            &[
                "remove",
                "--no-delete-branch",
                "--no-hooks",
                "--foreground",
                "--format=json",
                branch,
            ],
        )?;
        if output.status.success() {
            validate_cleaned(checked_json(output)?, worktree, branch)?;
            return Ok(CleanOutcome {
                cleaned: true,
                reason: None,
            });
        }
        Ok(CleanOutcome {
            cleaned: false,
            reason: Some(bounded_reason(&output.stderr)),
        })
    }
}

#[derive(Deserialize)]
struct SwitchAnswer {
    branch: Option<String>,
    path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BranchLocation {
    Missing,
    BranchOnly,
    Worktree(String),
}

fn validate_target(repository: &Path, branch: &str) -> Result<()> {
    if !repository.is_absolute() || !repository.is_dir() {
        return Err(errors::invalid(
            "Repository must be an existing absolute directory",
        ));
    }
    if branch.is_empty()
        || branch.len() > 160
        || branch.starts_with('-')
        || !branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_'))
    {
        return Err(errors::invalid("Invalid worktree branch"));
    }
    Ok(())
}

fn bounded_command(command: Command, timeout: Duration) -> Result<Output> {
    soyeht_process::run(command, Limits::new(timeout, OUTPUT_LIMIT))
        .map_err(errors::worktree_process)
}

fn checked_json(output: Output) -> Result<Value> {
    if !output.status.success() {
        return Err(errors::worktree_unavailable(bounded_reason(&output.stderr)));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| errors::worktree_unavailable("Invalid worktree response"))
}

fn bounded_reason(bytes: &[u8]) -> String {
    let bytes = &bytes[..bytes.len().min(4096)];
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn find_branch(value: &Value, branch: &str) -> Option<BranchLocation> {
    branch_items(value)
        .find(|item| item.get("branch").and_then(Value::as_str) == Some(branch))
        .map(branch_location)
}

fn branch_items(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .into_iter()
        .flatten()
}

fn branch_location(item: &Value) -> BranchLocation {
    item.pointer("/worktree/path")
        .or_else(|| item.get("path"))
        .and_then(Value::as_str)
        .map_or(BranchLocation::BranchOnly, |path| {
            BranchLocation::Worktree(path.into())
        })
}

fn cleanup_refusal(listing: &Value, branch: &str) -> Result<Option<String>> {
    let item = branch_items(listing)
        .find(|item| item.get("branch").and_then(Value::as_str) == Some(branch))
        .ok_or_else(|| errors::worktree_unavailable("Worktree listing lost the task branch"))?;
    let changes = item
        .pointer("/worktree/changes")
        .or_else(|| item.get("working_tree"))
        .and_then(Value::as_object);
    let Some(changes) = changes else {
        return Ok(Some("Worktree cleanliness is unknown".into()));
    };
    if ["staged", "modified", "untracked", "renamed", "deleted"]
        .into_iter()
        .any(|name| changes.get(name).and_then(Value::as_bool) != Some(false))
    {
        return Ok(Some("Worktree has uncommitted changes".into()));
    }
    if !head_reachable(listing, item, branch) {
        return Ok(Some(
            "Worktree HEAD is not reachable from a branch outside the worktree".into(),
        ));
    }
    Ok(None)
}

fn head_reachable(listing: &Value, target: &Value, branch: &str) -> bool {
    let integration = target
        .pointer("/default_branch/integration/reason")
        .or_else(|| target.get("integration_reason"))
        .and_then(Value::as_str);
    if matches!(
        integration,
        Some("same_commit" | "same-commit" | "ancestor")
    ) {
        return true;
    }
    let upstream = target.get("upstream").or_else(|| target.get("remote"));
    if upstream
        .and_then(|value| value.get("ahead"))
        .and_then(Value::as_u64)
        == Some(0)
        && upstream
            .and_then(|value| value.get("branch"))
            .and_then(Value::as_str)
            .is_some()
    {
        return true;
    }
    let head = target
        .pointer("/head/sha")
        .or_else(|| target.pointer("/commit/sha"));
    head.is_some_and(|head| {
        branch_items(listing).any(|item| {
            item.get("branch").and_then(Value::as_str) != Some(branch)
                && item
                    .pointer("/head/sha")
                    .or_else(|| item.pointer("/commit/sha"))
                    == Some(head)
        })
    })
}

/// Windows has no way to ask which processes sit in a folder, so a cleanup there stops with a
/// clear answer instead of removing a folder a process may be using.
#[cfg(windows)]
fn process_uses(_worktree: &Path) -> Result<bool> {
    Err(errors::worktree_unavailable(
        "Finding the processes that use a worktree is not supported on Windows",
    ))
}

#[cfg(unix)]
fn process_uses(worktree: &Path) -> Result<bool> {
    let mut command = Command::new(crate::web::lsof_program());
    command.args(["-a", "-d", "cwd", "-Fp", "+D"]).arg(worktree);
    let output = bounded_command(command, PROCESS_SCAN_TIMEOUT)?;
    if output.stdout.contains(&b'p') {
        return Ok(true);
    }
    if output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(false);
    }
    Err(errors::worktree_unavailable(bounded_reason(&output.stderr)))
}

fn validate_cleaned(value: Value, worktree: &Path, branch: &str) -> Result<()> {
    if value.get("kind").and_then(Value::as_str) != Some("worktree")
        || value.get("branch").and_then(Value::as_str) != Some(branch)
        || value.get("path").and_then(Value::as_str) != worktree.to_str()
        || value.get("branch_outcome").and_then(Value::as_str) != Some("not_attempted")
    {
        return Err(errors::worktree_unavailable(
            "Worktree cleanup response changed the requested target",
        ));
    }
    Ok(())
}

fn refused(reason: impl Into<String>) -> Result<CleanOutcome> {
    Ok(CleanOutcome {
        cleaned: false,
        reason: Some(reason.into()),
    })
}

fn checked_prepared(path: String, branch: &str) -> Result<PreparedWorktree> {
    if !Path::new(&path).is_absolute() || !Path::new(&path).is_dir() {
        return Err(errors::worktree_unavailable(
            "Worktree returned a missing or relative path",
        ));
    }
    Ok(PreparedWorktree {
        path,
        branch: branch.into(),
    })
}

pub struct DispatchRequest<'request> {
    pub repository: &'request Path,
    pub task: &'request Task,
    pub subitems: &'request [Subitem],
    pub comments: &'request [Comment],
    pub preset: TaskExecutor,
    pub pane_id: String,
}

pub(crate) fn task_branch(task: &str) -> String {
    format!("task/{}", task.to_ascii_lowercase())
}

pub(crate) fn first_prompt(
    request: &DispatchRequest<'_>,
    checkpoint: &DispatchCheckpoint,
) -> Result<String> {
    let worktree = checkpoint
        .worktree_path
        .as_deref()
        .ok_or_else(|| errors::internal("Dispatch checkpoint has no worktree"))?;
    let verify = request
        .task
        .how_to_verify
        .as_deref()
        .unwrap_or("Not specified");
    let subitems = request
        .subitems
        .iter()
        .map(|item| {
            format!(
                "- [{}] {}",
                if item.state == soyeht_protocol::SubitemState::Done {
                    "x"
                } else {
                    " "
                },
                item.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let comments = request
        .comments
        .iter()
        .map(|comment| format!("- {}: {}", comment.author, comment.text))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "# {}\n\n{}\n\n## Must not change\n{}\n\n## How to verify\n{}\n\n## Subitems\n{}\n\n## Comments\n{}\n\n## Worktree\n{}\n\nCreate subitems before starting. Comment at each milestone. Request review when done. If blocked, state the blocking kind.",
        request.task.title,
        request.task.description,
        request.task.must_not_change,
        verify,
        if subitems.is_empty() {
            "- None"
        } else {
            &subitems
        },
        if comments.is_empty() {
            "- None"
        } else {
            &comments
        },
        worktree,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("sn-worktree-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn command_adapter_uses_machine_output_and_never_deletes_the_branch() {
        let directory = TestDirectory::new();
        let executable = directory.path().join("fake-wt");
        let worktree = directory.path().join("task-tree");
        let calls = directory.path().join("calls");
        let state = directory.path().join("created");
        fs::create_dir(&worktree).unwrap();
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \" $* \" in\n  *\" list \"*) if [ -e '{}' ]; then printf '%s' '{{\"schema\":2,\"items\":[{{\"branch\":\"task/ws-9\",\"head\":{{\"sha\":\"abc\"}},\"worktree\":{{\"path\":\"{}\",\"changes\":{{\"staged\":false,\"modified\":false,\"untracked\":false,\"renamed\":false,\"deleted\":false}}}},\"default_branch\":{{\"integration\":{{\"reason\":\"ancestor\"}}}}}}]}}'; else printf '%s' '{{\"schema\":2,\"items\":[]}}'; fi ;;\n  *\" switch \"*) touch '{}'; printf '%s' '{{\"action\":\"created\",\"branch\":\"task/ws-9\",\"path\":\"{}\"}}' ;;\n  *\" remove \"*) printf '%s' '{{\"kind\":\"worktree\",\"branch\":\"task/ws-9\",\"path\":\"{}\",\"branch_outcome\":\"not_attempted\"}}' ;;\nesac\n",
            calls.display(),
            state.display(),
            worktree.display(),
            state.display(),
            worktree.display(),
            worktree.display(),
        );
        fs::write(&executable, script).unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let mut adapter = CommandWorktree::using(executable.to_string_lossy().into());
        let prepared = adapter.prepare(directory.path(), "task/ws-9").unwrap();
        assert_eq!(prepared.path, worktree.to_string_lossy());
        assert!(
            adapter
                .clean(directory.path(), &worktree, "task/ws-9")
                .unwrap()
                .cleaned
        );
        let calls = fs::read_to_string(calls).unwrap();
        assert!(calls.contains("switch --no-cd --no-hooks --format=json --create task/ws-9"));
        assert!(
            calls.contains(
                "remove --no-delete-branch --no-hooks --foreground --format=json task/ws-9"
            )
        );
    }

    #[test]
    fn cleanup_refuses_dirty_and_unreachable_worktrees_before_removal() {
        let path = "/tmp/task-ws-9";
        let dirty = json!({"schema":2,"items":[{
            "branch":"task/ws-9","head":{"sha":"abc"},
            "worktree":{"path":path,"changes":{"staged":false,"modified":true,
                "untracked":false,"renamed":false,"deleted":false}},
            "default_branch":{"integration":{"reason":"ancestor"}}
        }]});
        assert_eq!(
            cleanup_refusal(&dirty, "task/ws-9").unwrap().as_deref(),
            Some("Worktree has uncommitted changes")
        );
        let unreachable = json!({"schema":2,"items":[{
            "branch":"task/ws-9","head":{"sha":"abc"},
            "worktree":{"path":path,"changes":{"staged":false,"modified":false,
                "untracked":false,"renamed":false,"deleted":false}}
        }]});
        assert_eq!(
            cleanup_refusal(&unreachable, "task/ws-9")
                .unwrap()
                .as_deref(),
            Some("Worktree HEAD is not reachable from a branch outside the worktree")
        );
    }
}
