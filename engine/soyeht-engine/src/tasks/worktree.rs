//! Worktree process boundary and resumable dispatch steps.

use crate::errors::{self, Result};
use serde::Deserialize;
use serde_json::Value;
use soyeht_process::{Limits, Output};
use soyeht_protocol::{Comment, Subitem, Task, TaskExecutor};
use soyeht_store::{database::Store, tasks::DispatchCheckpoint};
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
        .map_err(|error| errors::worktree_unavailable(error.to_string()))
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
    let mut command = Command::new("lsof");
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

pub trait DispatchJournal {
    fn checkpoint(&self, workspace: &str, task: &str) -> Result<Option<DispatchCheckpoint>>;
    fn save_checkpoint(
        &mut self,
        workspace: &str,
        task: &str,
        checkpoint: &DispatchCheckpoint,
    ) -> Result<()>;
}

impl DispatchJournal for Store {
    fn checkpoint(&self, workspace: &str, task: &str) -> Result<Option<DispatchCheckpoint>> {
        self.dispatch_checkpoint(workspace, task)
            .map_err(errors::storage)
    }

    fn save_checkpoint(
        &mut self,
        workspace: &str,
        task: &str,
        checkpoint: &DispatchCheckpoint,
    ) -> Result<()> {
        self.record_dispatch_checkpoint(workspace, task, checkpoint)
            .map_err(errors::storage)
    }
}

#[derive(Clone)]
pub struct AgentPaneSpec {
    pub pane_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub name: String,
    pub preset: TaskExecutor,
}

pub trait TaskPanePort {
    fn open(&mut self, spec: &AgentPaneSpec) -> Result<()>;
    fn send_first_prompt(&mut self, pane: &str, prompt_id: &str, text: &str) -> Result<()>;
}

pub struct DispatchRequest<'request> {
    pub repository: &'request Path,
    pub task: &'request Task,
    pub subitems: &'request [Subitem],
    pub comments: &'request [Comment],
    pub preset: TaskExecutor,
    pub pane_id: String,
}

pub fn resume_dispatch(
    journal: &mut impl DispatchJournal,
    worktrees: &mut impl WorktreePort,
    panes: &mut impl TaskPanePort,
    request: &DispatchRequest<'_>,
) -> Result<DispatchCheckpoint> {
    let workspace = &request.task.workspace_id;
    let task = &request.task.task_id;
    let branch = task_branch(task);
    let mut checkpoint = match journal.checkpoint(workspace, task)? {
        Some(checkpoint) => checkpoint,
        None => {
            let checkpoint = reserved_checkpoint(&request.pane_id, &branch);
            journal.save_checkpoint(workspace, task, &checkpoint)?;
            checkpoint
        }
    };
    validate_checkpoint(&checkpoint, &request.pane_id, &branch)?;
    if checkpoint.stage == "reserved" {
        let prepared = worktrees.prepare(request.repository, &branch)?;
        checkpoint.stage = "worktree_created".into();
        checkpoint.worktree_path = Some(prepared.path);
        checkpoint.branch = Some(prepared.branch);
        journal.save_checkpoint(workspace, task, &checkpoint)?;
    }
    if checkpoint.stage == "worktree_created" {
        panes.open(&pane_spec(request, &checkpoint)?)?;
        checkpoint.stage = "pane_opened".into();
        journal.save_checkpoint(workspace, task, &checkpoint)?;
    }
    if checkpoint.stage == "pane_opened" {
        let prompt_id = format!("task:{workspace}:{task}:{}", request.task.current_attempt);
        let prompt = first_prompt(request, &checkpoint)?;
        panes.send_first_prompt(&request.pane_id, &prompt_id, &prompt)?;
        checkpoint.stage = "prompt_accepted".into();
        journal.save_checkpoint(workspace, task, &checkpoint)?;
    }
    if checkpoint.stage != "prompt_accepted" {
        return Err(errors::internal("Unknown dispatch checkpoint stage"));
    }
    Ok(checkpoint)
}

pub(crate) fn task_branch(task: &str) -> String {
    format!("task/{}", task.to_ascii_lowercase())
}

fn reserved_checkpoint(pane: &str, branch: &str) -> DispatchCheckpoint {
    DispatchCheckpoint {
        stage: "reserved".into(),
        worktree_path: None,
        branch: Some(branch.into()),
        pane_id: Some(pane.into()),
    }
}

fn validate_checkpoint(checkpoint: &DispatchCheckpoint, pane: &str, branch: &str) -> Result<()> {
    if checkpoint.pane_id.as_deref() != Some(pane) || checkpoint.branch.as_deref() != Some(branch) {
        return Err(errors::internal(
            "Dispatch checkpoint does not match its task attempt",
        ));
    }
    Ok(())
}

fn pane_spec(
    request: &DispatchRequest<'_>,
    checkpoint: &DispatchCheckpoint,
) -> Result<AgentPaneSpec> {
    Ok(AgentPaneSpec {
        pane_id: request.pane_id.clone(),
        workspace_id: request.task.workspace_id.clone(),
        cwd: checkpoint
            .worktree_path
            .clone()
            .ok_or_else(|| errors::internal("Dispatch checkpoint has no worktree"))?,
        name: request.task.title.clone(),
        preset: request.preset.clone(),
    })
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
    use crate::tasks::rules::tests::task_fixture;
    use serde_json::json;
    use soyeht_protocol::TaskState;
    use std::{
        collections::BTreeMap,
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

    #[derive(Default)]
    struct MemoryJournal {
        value: Option<DispatchCheckpoint>,
        writes: Vec<String>,
    }

    impl DispatchJournal for MemoryJournal {
        fn checkpoint(&self, _: &str, _: &str) -> Result<Option<DispatchCheckpoint>> {
            Ok(self.value.clone())
        }

        fn save_checkpoint(
            &mut self,
            _: &str,
            _: &str,
            checkpoint: &DispatchCheckpoint,
        ) -> Result<()> {
            self.writes.push(checkpoint.stage.clone());
            self.value = Some(checkpoint.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeWorktree {
        calls: u32,
        path: String,
    }

    impl WorktreePort for FakeWorktree {
        fn prepare(&mut self, _: &Path, branch: &str) -> Result<PreparedWorktree> {
            self.calls += 1;
            Ok(PreparedWorktree {
                path: self.path.clone(),
                branch: branch.into(),
            })
        }

        fn clean(&mut self, _: &Path, _: &Path, _: &str) -> Result<CleanOutcome> {
            Ok(CleanOutcome {
                cleaned: true,
                reason: None,
            })
        }
    }

    #[derive(Default)]
    struct FakePanes {
        opens: BTreeMap<String, AgentPaneSpec>,
        prompts: BTreeMap<String, String>,
    }

    impl TaskPanePort for FakePanes {
        fn open(&mut self, spec: &AgentPaneSpec) -> Result<()> {
            self.opens
                .entry(spec.pane_id.clone())
                .or_insert_with(|| spec.clone());
            Ok(())
        }

        fn send_first_prompt(&mut self, _: &str, prompt_id: &str, text: &str) -> Result<()> {
            self.prompts
                .entry(prompt_id.into())
                .or_insert_with(|| text.into());
            Ok(())
        }
    }

    fn preset() -> TaskExecutor {
        TaskExecutor {
            workspace_default: false,
            harness: Some("test-acp".into()),
            model: Some("test-model".into()),
            effort: None,
            permission_mode: Some("ask".into()),
        }
    }

    #[test]
    fn coordinator_resumes_after_every_checkpoint_without_duplicate_effects() {
        let directory = TestDirectory::new();
        let mut task = task_fixture(TaskState::InProgress);
        task.current_attempt = 4;
        let request = DispatchRequest {
            repository: directory.path(),
            task: &task,
            subitems: &[],
            comments: &[],
            preset: preset(),
            pane_id: "pane-7".into(),
        };
        let mut journal = MemoryJournal::default();
        let mut worktrees = FakeWorktree {
            path: directory.path().to_string_lossy().into(),
            ..FakeWorktree::default()
        };
        let mut panes = FakePanes::default();

        let first = resume_dispatch(&mut journal, &mut worktrees, &mut panes, &request).unwrap();
        let second = resume_dispatch(&mut journal, &mut worktrees, &mut panes, &request).unwrap();
        assert_eq!(first.stage, "prompt_accepted");
        assert_eq!(second, first);
        assert_eq!(worktrees.calls, 1);
        assert_eq!(panes.opens.len(), 1);
        assert_eq!(panes.prompts.len(), 1);
        assert_eq!(
            journal.writes,
            [
                "reserved",
                "worktree_created",
                "pane_opened",
                "prompt_accepted"
            ]
        );
        let prompt = panes.prompts.values().next().unwrap();
        for required in [
            &task.title,
            &task.description,
            &task.must_not_change,
            directory.path().to_str().unwrap(),
        ] {
            assert!(prompt.contains(required));
        }
    }

    #[test]
    fn dispatch_after_worktree_checkpoint_starts_with_opening_the_pane() {
        let directory = TestDirectory::new();
        let task = task_fixture(TaskState::InProgress);
        let request = DispatchRequest {
            repository: directory.path(),
            task: &task,
            subitems: &[],
            comments: &[],
            preset: preset(),
            pane_id: "pane-7".into(),
        };
        let mut journal = MemoryJournal {
            value: Some(DispatchCheckpoint {
                stage: "worktree_created".into(),
                worktree_path: Some(directory.path().to_string_lossy().into()),
                branch: Some(task_branch(&task.task_id)),
                pane_id: Some("pane-7".into()),
            }),
            ..MemoryJournal::default()
        };
        let mut worktrees = FakeWorktree::default();
        let mut panes = FakePanes::default();

        let result = resume_dispatch(&mut journal, &mut worktrees, &mut panes, &request).unwrap();

        assert_eq!(result.stage, "prompt_accepted");
        assert_eq!(worktrees.calls, 0);
        assert_eq!(panes.opens.len(), 1);
        assert_eq!(panes.prompts.len(), 1);
        assert_eq!(journal.writes, ["pane_opened", "prompt_accepted"]);
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
