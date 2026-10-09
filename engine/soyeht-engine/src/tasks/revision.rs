//! Pull request status and repository-diff boundary for task rules.

use crate::{errors, errors::Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use soyeht_process::Output;
use soyeht_protocol::{TaskCheckState, TaskReviewState};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
/// A hung git (a held index lock, a stalled network mount) is killed after
/// this long and reported as `worktree_unavailable`.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    PublicFunction,
    Handler,
    ProtocolMessage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSurface {
    pub path: String,
    pub name: String,
    pub kind: SurfaceKind,
    pub patch: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevisionSnapshot {
    pub head: String,
    pub check_state: TaskCheckState,
    pub review_state: TaskReviewState,
    pub summary: String,
    pub log_tail: String,
    pub fingerprint: String,
    pub changed_paths: Vec<String>,
    pub changed_surfaces: Vec<ChangedSurface>,
}

/// Runs git. The engine calls it only off the Core lock (ADR 0079).
pub trait RevisionPort: Send {
    fn inspect(&mut self, repository: &Path) -> Result<RevisionSnapshot>;
}

#[derive(Default)]
pub struct GitRevision;

impl RevisionPort for GitRevision {
    fn inspect(&mut self, repository: &Path) -> Result<RevisionSnapshot> {
        let root = canonical_repository(repository)?;
        let base = base_commit(&root)?;
        let head = text(git(&root, &["rev-parse", "HEAD"])?)?;
        let paths = changed_paths(&root, &base)?;
        let (patch, changed_surfaces) = complete_patch(&root, &base, &paths)?;
        let fingerprint = format!("{:x}", Sha256::digest(patch.as_bytes()));
        Ok(RevisionSnapshot {
            head,
            check_state: TaskCheckState::Unknown,
            review_state: TaskReviewState::None,
            summary: format!("{} changed paths", paths.len()),
            log_tail: String::new(),
            fingerprint,
            changed_paths: paths,
            changed_surfaces,
        })
    }
}

fn canonical_repository(repository: &Path) -> Result<PathBuf> {
    if !repository.is_absolute() {
        return Err(errors::worktree_unavailable(
            "Review repository must be absolute",
        ));
    }
    repository
        .canonicalize()
        .map_err(|error| errors::worktree_unavailable(error.to_string()))
}

fn git(repository: &Path, args: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0");
    soyeht_process::run(
        command,
        soyeht_process::Limits::new(GIT_TIMEOUT, OUTPUT_LIMIT),
    )
    .map_err(|error| errors::worktree_unavailable(error.to_string()))
}

fn bytes(output: Output) -> Result<Vec<u8>> {
    if !output.status.success() {
        return Err(errors::worktree_unavailable(bounded(&output.stderr)));
    }
    Ok(output.stdout)
}

fn text(output: Output) -> Result<String> {
    String::from_utf8(bytes(output)?)
        .map(|value| value.trim().to_owned())
        .map_err(|_| errors::worktree_unavailable("Revision output is not UTF-8"))
}

fn bounded(stderr: &[u8]) -> String {
    String::from_utf8_lossy(&stderr[..stderr.len().min(4096)])
        .trim()
        .to_owned()
}

fn base_commit(repository: &Path) -> Result<String> {
    for reference in [
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
        "main",
    ] {
        let argument = format!("{reference}^{{commit}}");
        if let Ok(output) = git(repository, &["rev-parse", "--verify", &argument])
            && output.status.success()
        {
            let candidate = text(output)?;
            return text(git(repository, &["merge-base", "HEAD", &candidate])?);
        }
    }
    Err(errors::worktree_unavailable(
        "Repository has no local or remote main baseline",
    ))
}

fn changed_paths(repository: &Path, base: &str) -> Result<Vec<String>> {
    let mut paths = nul_paths(&bytes(git(
        repository,
        &["diff", "--name-only", "-z", base, "--", "."],
    )?)?)?;
    paths.extend(nul_paths(&bytes(git(
        repository,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?)?)?);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn nul_paths(raw: &[u8]) -> Result<Vec<String>> {
    raw.split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec())
                .map_err(|_| errors::worktree_unavailable("Revision path is not UTF-8"))
        })
        .collect()
}

fn complete_patch(
    repository: &Path,
    base: &str,
    paths: &[String],
) -> Result<(String, Vec<ChangedSurface>)> {
    let mut patch = String::new();
    let mut found = Vec::new();
    for path in paths {
        let current = path_patch(repository, base, path)?;
        found.extend(surfaces(path, &current));
        patch.push_str(path);
        patch.push('\0');
        patch.push_str(&current);
        if patch.len() > OUTPUT_LIMIT {
            return Err(errors::worktree_unavailable("Revision patch exceeds 4 MiB"));
        }
    }
    Ok((patch, found))
}

fn path_patch(repository: &Path, base: &str, path: &str) -> Result<String> {
    let candidate = repository.join(path);
    if candidate.is_file() && is_untracked(repository, path)? {
        let content = fs::read_to_string(&candidate)
            .map_err(|error| errors::worktree_unavailable(error.to_string()))?;
        let mut patch = format!("@@ -0,0 +1,{} @@\n", content.lines().count());
        for line in content.lines() {
            patch.push('+');
            patch.push_str(line);
            patch.push('\n');
        }
        return Ok(patch);
    }
    String::from_utf8(bytes(git(
        repository,
        &["diff", "--no-ext-diff", "--unified=200", base, "--", path],
    )?)?)
    .map_err(|_| errors::worktree_unavailable("Revision patch is not UTF-8"))
}

fn is_untracked(repository: &Path, path: &str) -> Result<bool> {
    let output = git(repository, &["ls-files", "--error-unmatch", "--", path])?;
    Ok(!output.status.success())
}

fn surfaces(path: &str, patch: &str) -> Vec<ChangedSurface> {
    let mut result = BTreeMap::new();
    let mut hunk = Vec::new();
    for line in patch.lines() {
        if line.starts_with("@@") {
            add_hunk_surfaces(path, &hunk, &mut result);
            hunk.clear();
            hunk.push(line.to_owned());
        } else if !diff_metadata(line) {
            hunk.push(line.to_owned());
        }
    }
    add_hunk_surfaces(path, &hunk, &mut result);
    result.into_values().collect()
}

fn diff_metadata(line: &str) -> bool {
    [
        "diff --git ",
        "index ",
        "new file ",
        "deleted file ",
        "similarity index ",
        "rename from ",
        "rename to ",
        "--- ",
        "+++ ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn add_hunk_surfaces(
    path: &str,
    lines: &[String],
    result: &mut BTreeMap<(String, String, SurfaceKind), ChangedSurface>,
) {
    if path.is_empty() || !lines.iter().any(|line| line.starts_with(['+', '-'])) {
        return;
    }
    let patch = lines.join("\n");
    for changed in lines
        .iter()
        .enumerate()
        .filter(|(_, line)| changed_line(line))
        .map(|(index, _)| index)
    {
        let surface = if path == "contracts/protocol.json" {
            protocol_surface(lines, changed)
        } else {
            enclosing_surface(path, lines, changed)
        };
        if let Some((name, kind)) = surface {
            let key = (path.to_owned(), name.clone(), kind.clone());
            result.entry(key).or_insert_with(|| ChangedSurface {
                path: path.to_owned(),
                name,
                kind,
                patch: patch.clone(),
            });
        }
    }
}

fn changed_line(line: &str) -> bool {
    (line.starts_with('+') && !line.starts_with("+++ "))
        || (line.starts_with('-') && !line.starts_with("--- "))
}

fn enclosing_surface(
    path: &str,
    lines: &[String],
    changed: usize,
) -> Option<(String, SurfaceKind)> {
    lines[..=changed]
        .iter()
        .rev()
        .find_map(|line| surface_name(path, source_line(line)))
        .or_else(|| {
            lines[changed + 1..]
                .iter()
                .find_map(|line| surface_name(path, source_line(line)))
        })
}

fn protocol_surface(lines: &[String], changed: usize) -> Option<(String, SurfaceKind)> {
    lines[..=changed]
        .iter()
        .rev()
        .find_map(|line| protocol_declaration(source_with_indent(line)))
        .or_else(|| {
            lines[changed + 1..]
                .iter()
                .find_map(|line| protocol_declaration(source_with_indent(line)))
        })
        .map(|name| (name, SurfaceKind::ProtocolMessage))
}

fn source_with_indent(line: &str) -> &str {
    match line.as_bytes().first() {
        Some(b'+' | b'-' | b' ') => &line[1..],
        _ => line,
    }
}

fn protocol_declaration(line: &str) -> Option<String> {
    (line.len() - line.trim_start().len() == 4)
        .then(|| quoted_object_name(line.trim_start()))
        .flatten()
}

fn source_line(line: &str) -> &str {
    source_with_indent(line).trim()
}

fn surface_name(path: &str, line: &str) -> Option<(String, SurfaceKind)> {
    let rust_public = line.starts_with("pub") && line.contains("fn ");
    let handler =
        is_handler_path(path) && (line.starts_with("fn ") || line.starts_with("async fn "));
    let swift_public = line.starts_with("public ") && line.contains("func ");
    let type_script =
        line.starts_with("export ") && (line.contains("function ") || line.contains("const "));
    let marker = if rust_public || handler {
        "fn "
    } else if swift_public {
        "func "
    } else if type_script && line.contains("function ") {
        "function "
    } else if type_script {
        "const "
    } else {
        return None;
    };
    let name = line.split_once(marker)?.1.trim_start();
    let name = name
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .next()?;
    (!name.is_empty()).then(|| {
        let kind = if is_handler_path(path) || name.to_ascii_lowercase().contains("handler") {
            SurfaceKind::Handler
        } else {
            SurfaceKind::PublicFunction
        };
        (name.into(), kind)
    })
}

fn is_handler_path(path: &str) -> bool {
    path.split('/').any(|part| part == "handlers")
        || path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with("handler"))
}

fn quoted_object_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix('"')?;
    let (name, tail) = rest.split_once('"')?;
    tail.trim_start()
        .starts_with(": {")
        .then(|| name.to_owned())
}

impl crate::core::Core {
    /// Runs git for one task off the Core lock (ADR 0079). The feedback is
    /// decided under the lock against the task as it is then.
    pub(super) fn inspect_task_revision(&mut self, task: soyeht_protocol::Task) -> Result<()> {
        let repository = super::review::review_repository(self, &task)?;
        let revision = self.revision.clone();
        let (workspace, task_id) = (task.workspace_id, task.task_id);
        self.spawn_background(
            format!("task_feedback:{workspace}:{task_id}"),
            move || revision.with(|port| port.inspect(&repository)),
            move |core, revision| {
                let revision = revision?;
                let task = core
                    .store
                    .tasks_with_pull_requests()
                    .map_err(errors::storage)?
                    .into_iter()
                    .find(|task| task.workspace_id == workspace && task.task_id == task_id);
                match task {
                    Some(task) => core.feedback(task, &revision),
                    None => Ok(()),
                }
            },
        )
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::errors;
    use std::{
        collections::{BTreeMap, VecDeque},
        fs,
        process::Command,
    };
    use uuid::Uuid;

    #[derive(Default)]
    pub struct ScriptedRevisionPort {
        scripts: BTreeMap<String, VecDeque<RevisionSnapshot>>,
    }

    impl ScriptedRevisionPort {
        pub fn script(&mut self, url: &str, snapshots: Vec<RevisionSnapshot>) {
            self.scripts.insert(url.into(), snapshots.into());
        }
    }

    impl RevisionPort for ScriptedRevisionPort {
        fn inspect(&mut self, repository: &Path) -> Result<RevisionSnapshot> {
            let url = repository.to_string_lossy();
            self.scripts
                .get_mut(url.as_ref())
                .and_then(VecDeque::pop_front)
                .ok_or_else(|| errors::internal("Revision fixture has no scripted result"))
        }
    }

    #[test]
    fn scripted_port_returns_check_and_review_states_in_order() {
        let url = "https://example.invalid/change/1";
        let mut port = ScriptedRevisionPort::default();
        let snapshot = |check_state, review_state, fingerprint: &str| RevisionSnapshot {
            head: "abc123".into(),
            check_state,
            review_state,
            summary: "A scripted result".into(),
            log_tail: "last line".into(),
            fingerprint: fingerprint.into(),
            changed_paths: Vec::new(),
            changed_surfaces: Vec::new(),
        };
        port.script(
            url,
            vec![
                snapshot(TaskCheckState::Red, TaskReviewState::None, "red-1"),
                snapshot(
                    TaskCheckState::Green,
                    TaskReviewState::AwaitingPerson,
                    "green-1",
                ),
            ],
        );

        assert_eq!(
            port.inspect(Path::new(url)).unwrap().check_state,
            TaskCheckState::Red
        );
        let green = port.inspect(Path::new(url)).unwrap();
        assert_eq!(green.check_state, TaskCheckState::Green);
        assert_eq!(green.review_state, TaskReviewState::AwaitingPerson);
        assert!(port.inspect(Path::new(url)).is_err());
    }

    struct Repository(PathBuf);

    impl Repository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("sn-review-{}", Uuid::new_v4()));
            fs::create_dir_all(path.join("src/handlers")).unwrap();
            run(&path, &["init", "-b", "main"]);
            run(&path, &["config", "user.name", "Test"]);
            run(&path, &["config", "user.email", "test@example.invalid"]);
            fs::write(
                path.join("src/lib.rs"),
                "pub(crate) fn calculate(value: u32) -> u32 {\n    value + 1\n}\n\npub fn untouched() {}\n",
            )
            .unwrap();
            fs::write(path.join("contracts.json"), "{\n  \"unchanged\": true\n}\n").unwrap();
            run(&path, &["add", "."]);
            run(&path, &["commit", "-m", "baseline"]);
            run(&path, &["switch", "-c", "feature"]);
            Self(path)
        }
    }

    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn run(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn git_adapter_finds_only_touched_public_and_handler_surfaces_with_a_stable_fingerprint() {
        let repository = Repository::new();
        fs::write(
            repository.0.join("src/lib.rs"),
            "pub(crate) fn calculate(value: u32) -> u32 {\n    value + 2\n}\n\npub fn untouched() {}\n",
        )
        .unwrap();
        fs::write(
            repository.0.join("src/handlers/save.rs"),
            "async fn save(request: Request) {\n    persist(request);\n}\n",
        )
        .unwrap();

        let first = GitRevision.inspect(&repository.0).unwrap();
        let second = GitRevision.inspect(&repository.0).unwrap();

        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(
            first.changed_paths,
            vec!["src/handlers/save.rs", "src/lib.rs"]
        );
        assert!(first.changed_surfaces.iter().any(|surface| {
            surface.path == "src/lib.rs"
                && surface.name == "calculate"
                && surface.kind == SurfaceKind::PublicFunction
        }));
        assert!(
            !first
                .changed_surfaces
                .iter()
                .any(|surface| surface.name == "untouched")
        );
        assert!(first.changed_surfaces.iter().any(|surface| {
            surface.path == "src/handlers/save.rs"
                && surface.name == "save"
                && surface.kind == SurfaceKind::Handler
        }));
    }

    #[test]
    fn protocol_change_maps_to_its_four_space_schema_object() {
        let patch = "diff --git a/contracts/protocol.json b/contracts/protocol.json\n@@ -1,7 +1,7 @@\n     \"TaskCompleteParams\": {\n       \"type\": \"object\",\n       \"properties\": {\n-        \"summary\": {\"type\": \"string\"}\n+        \"summary\": {\"type\": \"string\", \"minLength\": 1}\n       }\n     },\n     \"TaskReleaseParams\": {";
        let found = surfaces("contracts/protocol.json", patch);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "TaskCompleteParams");
        assert_eq!(found[0].kind, SurfaceKind::ProtocolMessage);
    }

    #[test]
    fn trusted_path_is_independent_of_diff_header_quoting_and_renames() {
        let found = surfaces(
            "src/new name.rs",
            "diff --git a/src/old.rs b/src/new.rs\n@@ -1 +1 @@\n-pub fn old() {}\n+pub fn new() {}",
        );
        assert!(
            found
                .iter()
                .all(|surface| surface.path == "src/new name.rs")
        );
    }
}
