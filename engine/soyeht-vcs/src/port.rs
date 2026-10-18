use crate::{Error, Result};
use soyeht_platform::git::Probe;
use soyeht_process::Output;
use std::{
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
    process::Command,
    time::Duration,
};

const COMMAND_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
/// A hung git (a stuck lock, a network filesystem) is killed after this long.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

pub trait CommandPort: Send {
    fn repository_root(&mut self, workspace: &Path) -> Result<PathBuf>;
    fn head_file(&mut self, repository: &Path, path: &Path) -> Result<Option<String>>;
    fn blame(&mut self, repository: &Path, path: &Path) -> Result<String>;
}

#[derive(Default)]
pub struct SystemGit;

impl CommandPort for SystemGit {
    fn repository_root(&mut self, workspace: &Path) -> Result<PathBuf> {
        let workspace =
            soyeht_platform::fs::canonicalize(workspace).map_err(|_| Error::InvalidRepository)?;
        let output = run(
            &workspace,
            [
                OsString::from("rev-parse"),
                OsString::from("--show-toplevel"),
            ],
        )?;
        let root = text(output)?.trim().to_owned();
        let root = soyeht_platform::fs::canonicalize(Path::new(&root))
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

fn run<I, S>(repository: &Path, arguments: I) -> Result<Output>
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

fn run_allow_failure<I, S>(repository: &Path, arguments: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_PAGER", "cat");
    run_git(&Probe::system(), command)
}

fn run_git(probe: &Probe, command: Command) -> Result<Output> {
    soyeht_process::run_git_with(
        probe,
        command,
        soyeht_process::Limits::new(COMMAND_TIMEOUT, COMMAND_OUTPUT_BYTES),
    )
    .map_err(|error| match error {
        soyeht_process::Error::GitUnavailable => Error::GitUnavailable,
        soyeht_process::Error::Spawn { .. } | soyeht_process::Error::UnderLock { .. } => {
            Error::CommandUnavailable
        }
        soyeht_process::Error::TimedOut { .. } => Error::TimedOut,
        soyeht_process::Error::OutputTooLarge { .. } => Error::OutputTooLarge,
        soyeht_process::Error::Wait { .. }
        | soyeht_process::Error::Read { .. }
        | soyeht_process::Error::Write { .. } => Error::CommandFailed,
    })
}

fn text(output: Output) -> Result<String> {
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

    #[cfg(unix)]
    #[test]
    fn git_is_not_started_without_the_command_line_tools() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("soyeht-vcs-no-clt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        let launcher = root.join("usr/bin/git");
        let marker = root.join("ran");
        fs::write(
            &launcher,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        let probe = Probe {
            launcher: Some(launcher),
            path: root.join("usr/bin").into_os_string(),
            developer_dir: None,
            select_link: root.join("xcode_select_link"),
            default_tools: root.join("Library/CommandLineTools"),
        };
        let result = run_git(&probe, Command::new("git"));
        let started = marker.exists();
        fs::remove_dir_all(&root).unwrap();
        assert!(matches!(result, Err(Error::GitUnavailable)));
        assert!(!started, "the launcher must never start");
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
