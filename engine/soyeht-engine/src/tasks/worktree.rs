//! Worktree process boundary and the first prompt of a dispatch.

use super::feedback::bounded;
use crate::errors::{self, Result};
use serde::Deserialize;
use serde_json::Value;
use soyeht_process::{Limits, Output};
use soyeht_protocol::{Comment, Subitem, Task, TaskExecutor, limits};
use soyeht_store::tasks::DispatchCheckpoint;
use std::{ffi::OsStr, path::Path, process::Command, time::Duration};

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
            executable: soyeht_platform::worktrunk::PROGRAM.into(),
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
        let path = crate::agents::environment::launch_search_path();
        self.wt_on(&path, repository, args, timeout)
    }

    /// Runs `wt` and the git it starts with `path` as their search path: the launch `PATH`, so an
    /// engine started at login finds the `wt` the person's shell finds (ADR 0135).
    fn wt_on(
        &self,
        path: &OsStr,
        repository: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<Output> {
        // `wt` starts git itself, so the check comes before `wt` does.
        let probe = soyeht_platform::git::Probe {
            path: path.to_owned(),
            ..soyeht_platform::git::Probe::system()
        };
        soyeht_process::require_git_with(&probe).map_err(errors::worktree_process)?;
        // `wt` runs git on the repository, and git starts the programs the repository's own
        // configuration names (the file system monitor, a filter driver, a hook). The settings that
        // keep git from it travel in the environment `wt` hands to every git it starts.
        let limits = Limits::new(LIST_TIMEOUT, OUTPUT_LIMIT);
        // Git before 2.31 ignores the settings, which would leave `wt` unhardened: it is not run.
        soyeht_process::require_config_environment(&probe, limits)
            .map_err(errors::worktree_process)?;
        let git = soyeht_process::ReadOnlyGit::prepare_with(&probe, repository, limits)
            .map_err(errors::worktree_process)?;
        let mut command = Command::new(&self.executable);
        command
            .env("PATH", path)
            .arg("-C")
            .arg(repository)
            .args(args);
        git.harden_environment(&mut command);
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

/// The room the first prompt gives to one line or field written by a person or an agent, as a
/// share of the prompt limit, so the fixed frame stays far below it.
const FIELD_BYTES: usize = limits::PROMPT_BYTES / 16;
const DESCRIPTION_BYTES: usize = limits::PROMPT_BYTES / 4;
const SUBITEMS_BYTES: usize = limits::PROMPT_BYTES / 8;
/// The comments a first prompt lists at most; the caller loads one more to learn that there
/// are older ones.
pub(crate) const PROMPT_COMMENTS: usize = 200;
const COMMENT_BYTES: usize = limits::PROMPT_BYTES / 32;
/// Kept free after the comments for the line that counts the ones left out.
const OMISSION_RESERVE: usize = 64;
const TRUNCATED: &str = "... [truncated]";
const INSTRUCTIONS: &str = "Create subitems before starting. Comment at each milestone. Request review when done. If blocked, state the blocking kind.";

/// The first prompt of a dispatched task, never longer than `limits::PROMPT_BYTES`. The frame
/// (title, must-not-change, how to verify, worktree, instructions) comes first and whole; the
/// description and the subitems are cut to their share; the comments, newest first, take what
/// is left, and a line says how many older ones did not fit.
pub(crate) fn first_prompt(
    request: &DispatchRequest<'_>,
    checkpoint: &DispatchCheckpoint,
) -> Result<String> {
    let worktree = checkpoint
        .worktree_path
        .as_deref()
        .ok_or_else(|| errors::internal("Dispatch checkpoint has no worktree"))?;
    let task = request.task;
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
    let head = format!(
        "# {}\n\n{}\n\n## Must not change\n{}\n\n## How to verify\n{}\n\n## Subitems\n{}\n\n## Comments\n",
        clipped(&task.title, FIELD_BYTES),
        clipped(&task.description, DESCRIPTION_BYTES),
        clipped(&task.must_not_change, FIELD_BYTES),
        clipped(
            task.how_to_verify.as_deref().unwrap_or("Not specified"),
            FIELD_BYTES
        ),
        if subitems.is_empty() {
            "- None".into()
        } else {
            clipped(&subitems, SUBITEMS_BYTES)
        },
    );
    let tail = format!(
        "\n\n## Worktree\n{}\n\n{INSTRUCTIONS}",
        clipped(worktree, FIELD_BYTES)
    );
    let room = limits::PROMPT_BYTES.saturating_sub(head.len() + tail.len());
    Ok(format!(
        "{head}{}{tail}",
        comments_section(request.comments, room)
    ))
}

/// `text` cut on a character boundary to at most `limit` bytes, ending in a marker when cut.
fn clipped(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.into();
    }
    let kept = bounded(text, limit.saturating_sub(TRUNCATED.len()));
    format!("{kept}{TRUNCATED}")
}

/// The comments (newest first, as given) that fit in `room` bytes, each cut to its share, then
/// the count of those left out.
fn comments_section(comments: &[Comment], room: usize) -> String {
    if comments.is_empty() {
        return "- None".into();
    }
    let mut section = String::new();
    let mut kept = 0;
    for comment in comments.iter().take(PROMPT_COMMENTS) {
        let line = clipped(
            &format!("- {}: {}", comment.author, comment.text),
            COMMENT_BYTES,
        );
        let separator = usize::from(!section.is_empty());
        let reserve = if kept + 1 < comments.len() {
            OMISSION_RESERVE
        } else {
            0
        };
        if section.len() + separator + line.len() + reserve > room {
            break;
        }
        if separator == 1 {
            section.push('\n');
        }
        section.push_str(&line);
        kept += 1;
    }
    let omitted = comments.len() - kept;
    if omitted > 0 {
        if !section.is_empty() {
            section.push('\n');
        }
        let plural = if omitted == 1 { "comment" } else { "comments" };
        // More than the cap were loaded: the count is what is known, not all there is.
        let count = if comments.len() > PROMPT_COMMENTS {
            format!("at least {omitted}")
        } else {
            omitted.to_string()
        };
        section.push_str(&format!("- {count} earlier {plural} omitted"));
    }
    section
}

#[cfg(all(test, unix))]
mod hardening_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn comment(n: usize, text: String) -> Comment {
        Comment {
            comment_id: format!("c{n}"),
            task_id: "WS-1".into(),
            author: "you".into(),
            text,
            at: "2026-09-11T00:00:00Z".into(),
        }
    }

    fn prompt_for(task: &Task, comments: &[Comment]) -> String {
        let request = DispatchRequest {
            repository: Path::new("/"),
            task,
            subitems: &[],
            comments,
            preset: TaskExecutor {
                workspace_default: true,
                harness: None,
                model: None,
                effort: None,
                permission_mode: None,
            },
            pane_id: "pane-1".into(),
        };
        let checkpoint = DispatchCheckpoint {
            stage: "pane_opened".into(),
            worktree_path: Some("/work/ws-1".into()),
            branch: Some("task/ws-1".into()),
            pane_id: Some("pane-1".into()),
        };
        first_prompt(&request, &checkpoint).unwrap()
    }

    #[test]
    fn a_small_task_keeps_every_section_and_every_comment() {
        let task = crate::tasks::task_fixture(soyeht_protocol::TaskState::InProgress);
        let text = prompt_for(
            &task,
            &[comment(2, "newer".into()), comment(1, "older".into())],
        );
        assert!(
            text.contains("## Comments\n- you: newer\n- you: older\n\n## Worktree\n/work/ws-1")
        );
        assert!(!text.contains("omitted"));
        assert!(text.ends_with(INSTRUCTIONS));
    }

    #[test]
    fn the_first_prompt_stays_within_the_limit_and_keeps_the_newest_comments() {
        let task = crate::tasks::task_fixture(soyeht_protocol::TaskState::InProgress);
        let comments: Vec<Comment> = (0..210)
            .rev()
            .map(|n| comment(n, format!("note {n} {}", "x".repeat(400))))
            .collect();
        let text = prompt_for(&task, &comments);
        assert!(text.len() <= limits::PROMPT_BYTES, "{}", text.len());
        assert!(text.contains("note 209 "), "the newest comment is kept");
        assert!(!text.contains("note 0 "), "the oldest comment is left out");
        let omitted = text
            .lines()
            .find_map(|line| {
                line.strip_prefix("- at least ")?
                    .strip_suffix(" earlier comments omitted")
            })
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert!(omitted > 0 && omitted < 210);
        assert_eq!(
            text.matches("- you: note ").count() + omitted,
            210,
            "every comment is either listed or counted"
        );
        assert!(text.contains("## Worktree\n/work/ws-1\n\nCreate subitems"));
    }

    #[test]
    fn the_omission_line_says_at_least_only_when_more_comments_exist_than_were_loaded() {
        let task = crate::tasks::task_fixture(soyeht_protocol::TaskState::InProgress);
        let comments = |count: usize| -> Vec<Comment> {
            (0..count)
                .rev()
                .map(|n| comment(n, format!("note {n} {}", "x".repeat(400))))
                .collect()
        };
        // The cap itself: every comment loaded, so the count of those left out is exact.
        let exact = prompt_for(&task, &comments(PROMPT_COMMENTS));
        assert!(exact.contains(" earlier comments omitted"));
        assert!(!exact.contains("at least"));
        // One more than the cap: there are older comments that were not even loaded.
        let more = prompt_for(&task, &comments(PROMPT_COMMENTS + 1));
        assert!(more.contains("- at least "));
        assert!(more.len() <= limits::PROMPT_BYTES);
        // Few enough comments to list every one: no line at all.
        assert!(!prompt_for(&task, &comments(3)).contains("omitted"));
    }

    #[test]
    fn oversized_fields_are_cut_on_a_character_boundary_with_a_marker() {
        let mut task = crate::tasks::task_fixture(soyeht_protocol::TaskState::InProgress);
        let wide = "\u{20ac}".repeat(limits::PROMPT_BYTES);
        task.title.clone_from(&wide);
        task.description.clone_from(&wide);
        task.must_not_change.clone_from(&wide);
        task.how_to_verify = Some(wide.clone());
        let huge = [comment(1, wide.clone()), comment(0, wide)];
        let text = prompt_for(&task, &huge);
        assert!(text.len() <= limits::PROMPT_BYTES, "{}", text.len());
        assert!(text.contains(TRUNCATED));
        assert!(text.ends_with(INSTRUCTIONS));
    }

    #[test]
    fn the_engine_runs_the_programs_name_on_this_system() {
        let port = CommandWorktree::default();
        assert_eq!(port.executable, soyeht_platform::worktrunk::PROGRAM);
        assert!(
            !cfg!(windows) || port.executable != "wt",
            "that is Windows Terminal"
        );
    }

    // The one test that needs a directory writes a shell script as the fake program, so the
    // directory helper exists on Unix only, as the test does.
    #[cfg(unix)]
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    #[cfg(unix)]
    struct TestDirectory(PathBuf);

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    /// An engine started at login has the user manager's `PATH`, which lacks the folder a login
    /// shell adds. `wt` is found on the launch `PATH` alone (ADR 0135).
    #[cfg(unix)]
    #[test]
    fn wt_is_found_on_the_launch_path_when_the_process_path_lacks_it() {
        let folder = TestDirectory::new();
        let wt = folder.path().join("wt");
        fs::write(&wt, "#!/bin/sh\necho '{\"schema\":2,\"items\":[]}'\n").unwrap();
        let mut permissions = fs::metadata(&wt).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&wt, permissions).unwrap();
        let process_path = std::env::var_os("PATH").unwrap_or_default();
        assert!(
            !std::env::split_paths(&process_path).any(|entry| entry == folder.path()),
            "the process PATH does not name the folder"
        );
        let adapter = CommandWorktree::using(soyeht_platform::worktrunk::PROGRAM.into());
        let repository = TestDirectory::new();
        // A folder that holds git and nothing else, so the only thing a search path can lack is
        // `wt`: git is on both paths below, and only one of them has the fake `wt`.
        let git = std::env::split_paths(&process_path)
            .map(|directory| directory.join("git"))
            .find(|candidate| candidate.is_file())
            .unwrap();
        let git_only = TestDirectory::new();
        std::os::unix::fs::symlink(&git, git_only.path().join("git")).unwrap();
        let without = std::env::join_paths([git_only.path()]).unwrap();
        let with = std::env::join_paths([folder.path(), git_only.path()]).unwrap();
        let probe = soyeht_platform::git::Probe {
            path: without.clone(),
            ..soyeht_platform::git::Probe::system()
        };
        soyeht_process::require_git_with(&probe).expect("git is found on the path without `wt`");
        let found = adapter.wt_on(&with, repository.path(), &["list"], LIST_TIMEOUT);
        assert!(found.unwrap().status.success());
        // The git check before it passed (above), so it is `wt` that cannot start.
        let missing = adapter.wt_on(&without, repository.path(), &["list"], LIST_TIMEOUT);
        assert!(
            missing.is_err(),
            "no `wt` on a path that has git and not the folder"
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
