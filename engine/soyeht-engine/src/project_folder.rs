//! The folder of a new project (ADR 0166 section 1).
//!
//! A workspace is a folder, and a project that is not an app or a repository has none: a book being
//! read, personal finances, a school subject. `project.folder.create` makes it under the data
//! directory's `projects` folder, named from the project's name, and answers with its path, which
//! the client then gives to `workspace.create`. The folder is empty and the person's: the engine
//! writes nothing in it, and removes nothing from it when the workspace is closed.

use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::{Value, json};
use soyeht_platform::fs::{canonicalize, ensure_private_dir};
use soyeht_protocol::{ProjectFolderCreateParams, limits, method};
use std::path::{Path, PathBuf};

/// The folder of the data directory that holds one folder per project.
const PROJECTS_DIRECTORY: &str = "projects";
/// The longest folder name made from a project's name, in characters.
const FOLDER_NAME_CHARS: usize = 48;
/// The folder name of a project whose name has no letter or digit.
const FALLBACK_NAME: &str = "project";
/// How many folders of one name `project.folder.create` tells apart before it gives up.
const SAME_NAME_FOLDERS: usize = 10_000;
/// Names Windows keeps for devices: a folder of one of them cannot be made there.
const WINDOWS_DEVICE_NAMES: [&str; 6] = ["con", "prn", "aux", "nul", "com", "lpt"];

/// The folder name a project name makes: lower-case letters and digits, one hyphen between the
/// words, at most [`FOLDER_NAME_CHARS`] characters.
fn folder_name(name: &str) -> String {
    let mut folder = String::new();
    let mut pending_hyphen = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if pending_hyphen && !folder.is_empty() {
                folder.push('-');
            }
            pending_hyphen = false;
            folder.push(character);
        } else {
            pending_hyphen = true;
        }
    }
    let mut folder: String = folder.chars().take(FOLDER_NAME_CHARS).collect();
    while folder.ends_with('-') {
        folder.pop();
    }
    if folder.is_empty() {
        return FALLBACK_NAME.into();
    }
    let device = WINDOWS_DEVICE_NAMES.iter().any(|device| {
        folder == *device
            || folder.strip_prefix(device).is_some_and(
                |rest| matches!(rest.as_bytes(), [digit] if (b'1'..=b'9').contains(digit)),
            )
    });
    if device {
        folder = format!("{FALLBACK_NAME}-{folder}");
    }
    folder
}

/// Makes the first folder of `base`, `base-2`, `base-3`... that does not exist yet, private to the
/// owner, and returns it.
fn make_folder(root: &Path, base: &str) -> Result<PathBuf> {
    for number in 1..=SAME_NAME_FOLDERS {
        let name = if number == 1 {
            base.to_owned()
        } else {
            format!("{base}-{number}")
        };
        let path = root.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(errors::io(error)),
        }
        ensure_private_dir(&path)
            .map_err(|_| errors::internal("Project folder could not be made private"))?;
        return Ok(path);
    }
    Err(errors::invalid("There are too many projects of that name"))
}

/// Makes the folder of a new project. The mkdir is one system call, so it stays under the lock;
/// nothing is walked.
pub(crate) fn create(core: &mut Core, connection: &str, params: &Value) -> Result<Value> {
    let request: ProjectFolderCreateParams = model::typed_params(params)?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > limits::PROJECT_NAME_CHARS {
        return Err(errors::invalid(format!(
            "A project's name is 1 to {} characters",
            limits::PROJECT_NAME_CHARS
        )));
    }
    let root = core.directory.join(PROJECTS_DIRECTORY);
    ensure_private_dir(&root).map_err(|_| errors::internal("Projects folder is not private"))?;
    let folder = make_folder(&root, &folder_name(name))?;
    let path = canonicalize(&folder)
        .map_err(errors::io)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| errors::internal("Project folder path must be UTF-8"))?;
    let result = json!({"path": path});
    let made = core.mutate(connection, method::PROJECT_FOLDER_CREATE, params, |_| {
        Ok((result, Vec::new()))
    });
    match made {
        Ok(mutation) => Ok(mutation.result),
        Err(error) => {
            // Nothing refers to the folder: it is empty and was made a moment ago.
            if let Err(removal) = std::fs::remove_dir(&folder) {
                soyeht_platform::log::failure("engine.project_folder_removal_failed", &removal);
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
