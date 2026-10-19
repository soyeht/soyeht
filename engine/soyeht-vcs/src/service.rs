use crate::{Blame, CommandPort, Diff, Error, Result, diff};
use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Clones share one command port, so a clone can inspect on a worker thread.
#[derive(Clone)]
pub struct Service {
    commands: Arc<Mutex<Box<dyn CommandPort>>>,
}

impl Service {
    pub fn new(commands: Box<dyn CommandPort>) -> Self {
        Self {
            commands: Arc::new(Mutex::new(commands)),
        }
    }

    /// Runs git, so the engine calls it off its Core lock.
    pub fn inspect(&self, workspace: &Path, path: &Path, current: &str) -> Result<Diff> {
        let mut commands = self
            .commands
            .lock()
            .map_err(|_| Error::CommandUnavailable)?;
        let root = match commands.repository_root(workspace) {
            Ok(root) => root,
            // A folder that is not a repository has no baseline to compare with: no hunks.
            Err(Error::InvalidRepository) => {
                return Ok(Diff {
                    hunks: Vec::new(),
                    blame: Vec::new(),
                });
            }
            Err(error) => return Err(error),
        };
        let path = repository_path(&root, workspace, path)?;
        let Some(baseline) = commands.head_file(&root, &path)? else {
            return diff::build(current, "", vec![]);
        };
        let blame = parse_blame(&commands.blame(&root, &path)?)?;
        diff::build(current, &baseline, blame)
    }

    pub fn review(&self, current: &str, baseline: &str) -> Result<Diff> {
        diff::build(current, baseline, Vec::new())
    }

    pub fn accept_hunk(&self, current: &str, baseline: &str, hunk_id: &str) -> Result<String> {
        diff::accept(current, baseline, hunk_id)
    }

    pub fn reject_hunk(&self, current: &str, baseline: &str, hunk_id: &str) -> Result<String> {
        diff::reject(current, baseline, hunk_id)
    }
}

fn repository_path(repository: &Path, workspace: &Path, file: &Path) -> Result<PathBuf> {
    if file.as_os_str().is_empty()
        || file.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::InvalidPath);
    }
    let workspace =
        soyeht_platform::fs::canonicalize(workspace).map_err(|_| Error::InvalidRepository)?;
    let prefix = workspace
        .strip_prefix(repository)
        .map_err(|_| Error::InvalidRepository)?;
    Ok(prefix.join(file))
}

fn parse_blame(source: &str) -> Result<Vec<Blame>> {
    let mut result = Vec::new();
    let mut record: Option<BlameRecord> = None;
    for line in source.lines() {
        if let Some(header) = header(line) {
            record = Some(header?);
        } else if let Some(current) = record.as_mut() {
            if let Some(author) = line.strip_prefix("author ") {
                current.author = bounded(author, 256);
            } else if let Some(summary) = line.strip_prefix("summary ") {
                current.summary = bounded(summary, 1024);
            } else if line.starts_with('\t') {
                result.push(current.finish()?);
                record = None;
            }
        }
    }
    if record.is_some() {
        return Err(Error::MalformedBlame);
    }
    Ok(result)
}

fn header(line: &str) -> Option<Result<BlameRecord>> {
    let mut parts = line.split_ascii_whitespace();
    let commit = parts.next()?;
    let original = parts.next()?.parse::<u32>().ok()?;
    let final_line = parts.next()?.parse::<u32>().ok()?;
    let count = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1);
    if !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) || commit.len() > 128 {
        return Some(Err(Error::MalformedBlame));
    }
    Some(Ok(BlameRecord {
        commit: commit.into(),
        original,
        final_line,
        count,
        author: String::new(),
        summary: String::new(),
    }))
}

fn bounded(value: &str, limit: usize) -> String {
    let end = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= limit)
        .last()
        .unwrap_or(0);
    value[..if value.len() <= limit {
        value.len()
    } else {
        end
    }]
        .to_owned()
}

struct BlameRecord {
    commit: String,
    original: u32,
    final_line: u32,
    count: u32,
    author: String,
    summary: String,
}

impl BlameRecord {
    fn finish(&self) -> Result<Blame> {
        if self.original == 0 || self.final_line == 0 || self.count == 0 || self.author.is_empty() {
            return Err(Error::MalformedBlame);
        }
        Ok(Blame {
            line: self.final_line - 1,
            commit: self.commit.clone(),
            author: self.author.clone(),
            summary: self.summary.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineKind, SystemGit};
    use std::{collections::VecDeque, fs, process::Command};

    struct FakeCommands {
        root: PathBuf,
        baseline: Option<String>,
        blame: String,
        calls: VecDeque<&'static str>,
    }

    impl CommandPort for FakeCommands {
        fn repository_root(&mut self, _: &Path) -> Result<PathBuf> {
            self.calls.push_back("root");
            Ok(self.root.clone())
        }

        fn head_file(&mut self, _: &Path, path: &Path) -> Result<Option<String>> {
            assert_eq!(path, Path::new("nested/src/lib.rs"));
            self.calls.push_back("head");
            Ok(self.baseline.clone())
        }

        fn blame(&mut self, _: &Path, path: &Path) -> Result<String> {
            assert_eq!(path, Path::new("nested/src/lib.rs"));
            self.calls.push_back("blame");
            Ok(self.blame.clone())
        }
    }

    #[test]
    fn fake_command_port_proves_subdirectory_mapping_and_blame() {
        let root = temporary("fake");
        let workspace = root.join("nested");
        fs::create_dir_all(&workspace).unwrap();
        let canonical_root = soyeht_platform::fs::canonicalize(&root).unwrap();
        let commands = FakeCommands {
            root: canonical_root,
            baseline: Some("one\ntwo\n".into()),
            blame: blame_fixture(),
            calls: VecDeque::new(),
        };
        let service = Service::new(Box::new(commands));
        let result = service
            .inspect(&workspace, Path::new("src/lib.rs"), "one\nchanged\n")
            .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.blame.len(), 1);
        assert_eq!(result.blame[0].line, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_repository_returns_buffer_diff_and_head_blame() {
        let root = temporary("real");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "one\ntwo\nthree\n").unwrap();
        command(&root, &["init", "-q"]);
        command(&root, &["add", "."]);
        command(
            &root,
            &[
                "-c",
                "user.name=Writer",
                "-c",
                "user.email=writer@example.invalid",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        let service = Service::new(Box::new(SystemGit));
        let result = service
            .inspect(
                &root,
                Path::new("src/lib.rs"),
                "one\nchanged\nthree\nfour\n",
            )
            .unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(
            result
                .blame
                .iter()
                .map(|item| item.line)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert!(result.blame.iter().all(|item| item.author == "Writer"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn untracked_file_is_an_all_added_diff_without_blame() {
        let root = temporary("untracked");
        fs::create_dir_all(&root).unwrap();
        command(&root, &["init", "-q"]);
        let service = Service::new(Box::new(SystemGit));
        let result = service
            .inspect(&root, Path::new("new.rs"), "new\n")
            .unwrap();
        assert_eq!(result.hunks[0].lines[0].kind, LineKind::Added);
        assert!(result.blame.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_a_repository_has_no_hunks_and_no_blame() {
        let root = temporary("no-repository");
        fs::create_dir_all(&root).unwrap();
        let service = Service::new(Box::new(SystemGit));
        let result = service
            .inspect(&root, Path::new("note.txt"), "one\ntwo\n")
            .unwrap();
        assert!(result.hunks.is_empty());
        assert!(result.blame.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    fn blame_fixture() -> String {
        format!(
            "{} 1 1 1\nauthor Writer\nsummary baseline\nfilename nested/src/lib.rs\n\tone\n{} 2 2 1\nauthor Writer\nsummary baseline\nfilename nested/src/lib.rs\n\ttwo\n",
            "a".repeat(40),
            "a".repeat(40)
        )
    }

    fn temporary(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("soyeht-vcs-{label}-{}", std::process::id()))
    }

    fn command(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
