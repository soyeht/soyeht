use crate::{Error, Result};
use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
};

const COMMAND_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

pub trait CommandPort: Send {
    fn repository_root(&mut self, workspace: &Path) -> Result<PathBuf>;
    fn head_file(&mut self, repository: &Path, path: &Path) -> Result<Option<String>>;
    fn blame(&mut self, repository: &Path, path: &Path) -> Result<String>;
}

#[derive(Default)]
pub struct SystemGit;

impl CommandPort for SystemGit {
    fn repository_root(&mut self, workspace: &Path) -> Result<PathBuf> {
        let workspace = workspace
            .canonicalize()
            .map_err(|_| Error::InvalidRepository)?;
        let output = run(
            &workspace,
            [
                OsString::from("rev-parse"),
                OsString::from("--show-toplevel"),
            ],
        )?;
        let root = text(output)?.trim().to_owned();
        let root = PathBuf::from(root)
            .canonicalize()
            .map_err(|_| Error::InvalidRepository)?;
        workspace
            .starts_with(&root)
            .then_some(root)
            .ok_or(Error::InvalidRepository)
    }

    fn head_file(&mut self, repository: &Path, path: &Path) -> Result<Option<String>> {
        validate_relative(path)?;
        if !has_head(repository)? || !head_has_path(repository, path)? {
            return Ok(None);
        }
        let spec = object_spec(path)?;
        let output = run(
            repository,
            [OsString::from("cat-file"), OsString::from("blob"), spec],
        )?;
        text(output).map(Some)
    }

    fn blame(&mut self, repository: &Path, path: &Path) -> Result<String> {
        validate_relative(path)?;
        text(run(
            repository,
            [
                OsString::from("blame"),
                OsString::from("--line-porcelain"),
                OsString::from("HEAD"),
                OsString::from("--"),
                path.as_os_str().to_owned(),
            ],
        )?)
    }
}

fn has_head(repository: &Path) -> Result<bool> {
    let output = run_allow_failure(
        repository,
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("HEAD^{commit}"),
        ],
    )?;
    Ok(output.status.success())
}

fn head_has_path(repository: &Path, path: &Path) -> Result<bool> {
    let output = run(
        repository,
        [
            OsString::from("ls-tree"),
            OsString::from("-z"),
            OsString::from("--full-tree"),
            OsString::from("HEAD"),
            OsString::from("--"),
            path.as_os_str().to_owned(),
        ],
    )?;
    Ok(!output.stdout.is_empty())
}

fn object_spec(path: &Path) -> Result<OsString> {
    let path = path.to_str().ok_or(Error::InvalidUtf8)?;
    Ok(OsString::from(format!("HEAD:{path}")))
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::InvalidPath);
    }
    Ok(())
}

fn run<I, S>(repository: &Path, arguments: I) -> Result<Captured>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_allow_failure(repository, arguments)?;
    output
        .status
        .success()
        .then_some(output)
        .ok_or(Error::CommandFailed)
}

fn run_allow_failure<I, S>(repository: &Path, arguments: I) -> Result<Captured>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new("git")
        .arg("--no-pager")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_PAGER", "cat")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Error::CommandUnavailable)?;
    let stdout = child.stdout.take().ok_or(Error::CommandUnavailable)?;
    let stderr = child.stderr.take().ok_or(Error::CommandUnavailable)?;
    let stdout = thread::spawn(move || read_limited(stdout, COMMAND_OUTPUT_BYTES));
    let stderr = thread::spawn(move || read_limited(stderr, 4096));
    let status = child.wait().map_err(|_| Error::CommandFailed)?;
    let stdout = stdout.join().map_err(|_| Error::CommandFailed)??;
    let stderr = stderr.join().map_err(|_| Error::CommandFailed)??;
    Ok(Captured {
        status,
        stdout,
        _stderr: stderr,
    })
}

fn read_limited(mut reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::CommandFailed)?;
    (bytes.len() <= limit)
        .then_some(bytes)
        .ok_or(Error::OutputTooLarge)
}

struct Captured {
    status: ExitStatus,
    stdout: Vec<u8>,
    _stderr: Vec<u8>,
}

fn text(output: Captured) -> Result<String> {
    String::from_utf8(output.stdout).map_err(|_| Error::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_validation_is_component_based() {
        assert!(validate_relative(Path::new("src/lib.rs")).is_ok());
        assert_eq!(
            validate_relative(Path::new("../outside")),
            Err(Error::InvalidPath)
        );
        assert_eq!(
            validate_relative(Path::new("/outside")),
            Err(Error::InvalidPath)
        );
        assert_eq!(validate_relative(Path::new("")), Err(Error::InvalidPath));
    }

    #[test]
    fn repository_root_requires_the_workspace_to_be_inside_git() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-vcs-no-repo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let result = SystemGit.repository_root(&directory);
        fs::remove_dir_all(&directory).unwrap();
        assert_eq!(result, Err(Error::CommandFailed));
    }
}
