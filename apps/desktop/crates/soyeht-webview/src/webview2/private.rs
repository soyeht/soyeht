//! The folders a private pane's page keeps what it needs in while it runs. Nothing the person
//! does in the pane is kept, but the browser's own files are, so each private page has a folder
//! of its own under the profile's `private` folder, made new for it, and the next start removes
//! the folders of programs that have ended.

use crate::{Error, Result};
use std::{
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
    core::HRESULT,
};

/// `FILE_ATTRIBUTE_REPARSE_POINT`: a junction or a link, which a folder of ours never is.
const REPARSE_POINT: u32 = 0x400;
/// What `GetExitCodeProcess` says of a process that has not ended.
const STILL_ACTIVE: u32 = 259;
/// How many names are tried for a folder that exists already.
const TRIES: u32 = 8;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn refuses(why: &'static str) -> Error {
    Error::Unavailable(why)
}

/// Whether `path` is a reparse point (or cannot be looked at), which is not trusted.
fn is_reparse_point(path: &Path) -> bool {
    std::fs::symlink_metadata(path).map_or(true, |meta| meta.file_attributes() & REPARSE_POINT != 0)
}

/// Makes the folder of a private page under `root`, named by this program's number and a count
/// that is new, and fails when the name is taken: a folder that was there before is not ours.
pub(super) fn make_folder(root: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(root).map_err(|_| refuses("the private web folder cannot be made"))?;
    if is_reparse_point(root) {
        return Err(refuses("the private web folder is not a plain folder"));
    }
    for _ in 0..TRIES {
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let folder = root.join(format!("{}-{count}", std::process::id()));
        match std::fs::create_dir(&folder) {
            Ok(()) if !is_reparse_point(&folder) => return Ok(folder),
            Ok(()) => return Err(refuses("the private web folder is not a plain folder")),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(refuses(
                    if error.kind() == std::io::ErrorKind::PermissionDenied {
                        "access to the private web folder was refused"
                    } else {
                        "the private web folder cannot be made"
                    },
                ));
            }
        }
    }
    Err(refuses("the private web folder cannot be made"))
}

/// Removes the folders under `root` of programs that have ended. A folder that is a link, or whose
/// program cannot be looked at, stays.
pub(super) fn remove_stale(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(process) = name
            .split_once('-')
            .and_then(|(pid, _)| pid.parse::<u32>().ok())
        else {
            continue;
        };
        let path = entry.path();
        if process != std::process::id()
            && !is_reparse_point(&path)
            && !process_is_alive(process)
            && std::fs::remove_dir_all(&path).is_err()
        {
            // A file of it is still held: the next start tries again.
        }
    }
}

/// Whether the process is still running. A process that has ended is not, though another program
/// may still hold a handle that keeps its number from going away; one that cannot be looked at
/// (access denied) is taken as running.
fn process_is_alive(process: u32) -> bool {
    // SAFETY: a request for a limited handle of a process, which the system may refuse.
    let opened = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) };
    match opened {
        Ok(handle) => {
            let mut code = 0_u32;
            // SAFETY: the handle was just made, and `code` is this call's own.
            let asked = unsafe { GetExitCodeProcess(handle, &raw mut code) };
            // SAFETY: the handle is closed once.
            if unsafe { CloseHandle(handle) }.is_err() {
                // A handle that cannot be closed is not one worth another word.
            }
            asked.is_err() || code == STILL_ACTIVE
        }
        // No such number: the program has ended.
        Err(error) => error.code() != HRESULT::from_win32(ERROR_INVALID_PARAMETER.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("soyeht-private-test-{name}-{}", std::process::id()))
    }

    #[test]
    fn each_private_page_gets_a_new_folder_named_for_this_program() {
        let root = scratch("new");
        let first = make_folder(&root).unwrap();
        let second = make_folder(&root).unwrap();
        assert_ne!(first, second);
        assert!(first.is_dir() && second.is_dir());
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("{}-", std::process::id()))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_that_was_made_before_is_not_used() {
        let root = scratch("taken");
        std::fs::create_dir_all(&root).unwrap();
        // The names the next tries would use are all taken, as by somebody who guessed them.
        let next = COUNTER.load(Ordering::Relaxed);
        for count in next..next + u64::from(TRIES) {
            std::fs::create_dir(root.join(format!("{}-{count}", std::process::id()))).unwrap();
        }
        assert!(make_folder(&root).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_of_a_program_that_ended_goes_and_one_that_runs_stays() {
        let root = scratch("stale");
        std::fs::create_dir_all(root.join("1999999999-0")).unwrap();
        let mine = make_folder(&root).unwrap();
        std::fs::create_dir_all(root.join("not-ours")).unwrap();
        remove_stale(&root);
        assert!(!root.join("1999999999-0").exists());
        assert!(mine.exists() && root.join("not-ours").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
