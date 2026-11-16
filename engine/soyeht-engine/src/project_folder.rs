//! The folder of a new project (ADR 0166 section 1).
//!
//! A workspace is a folder, and a project that is not an app or a repository has none: a book being
//! read, personal finances, a school subject. `project.folder.create` makes it under the data
//! directory's `projects` folder, named from the project's name, and answers with its path, which
//! the client then gives to `workspace.create`. The folder is empty and the person's: the engine
//! writes nothing in it, and removes nothing from it when the workspace is closed.
//!
//! A folder that is made and whose reply is then lost (a crash between the two) stays, empty. It is
//! the person's like any other, and nothing sweeps it: the engine cannot tell it from a folder the
//! person made by hand.

use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use icu_normalizer::ComposingNormalizerBorrowed;
use serde_json::Value;
use soyeht_platform::fs::{canonicalize, ensure_private_dir};
use soyeht_protocol::{ProjectFolderCreateParams, ProjectFolderCreateResult, limits, method};
use std::path::{Path, PathBuf};

/// The folder of the data directory that holds one folder per project.
const PROJECTS_DIRECTORY: &str = "projects";
/// The folder name of a project whose name has no letter or digit.
const FALLBACK_NAME: &str = "project";
/// How many folders of one name `project.folder.create` tells apart before it gives up.
const SAME_NAME_FOLDERS: usize = 10_000;
/// Names Windows keeps for devices, whatever follows the dot: `NUL`, `CON`, `PRN`, `AUX`.
const WINDOWS_BARE_DEVICES: [&str; 4] = ["con", "prn", "aux", "nul"];
/// The names Windows keeps for devices when a digit follows: `COM1`..`COM9`, `LPT1`..`LPT9`, the
/// superscript digits too.
const WINDOWS_NUMBERED_DEVICES: [&str; 2] = ["com", "lpt"];

fn is_device_name(folder: &str) -> bool {
    WINDOWS_BARE_DEVICES.contains(&folder)
        || WINDOWS_NUMBERED_DEVICES.iter().any(|device| {
            folder.strip_prefix(device).is_some_and(|rest| {
                let mut digits = rest.chars();
                matches!(
                    (digits.next(), digits.next()),
                    (Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
                )
            })
        })
}

/// The folder name a project name makes: composed (NFC) so a letter and its accent are one
/// character whichever way the client typed them, then lower-case letters and digits with one
/// hyphen between the words, at most `projectFolderNameChars` characters.
fn folder_name(name: &str) -> String {
    let composed = ComposingNormalizerBorrowed::new_nfc().normalize(name);
    let mut folder = String::new();
    let mut pending_hyphen = false;
    for character in composed.chars().flat_map(char::to_lowercase) {
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
    let mut folder: String = folder
        .chars()
        .take(limits::PROJECT_FOLDER_NAME_CHARS)
        .collect();
    while folder.ends_with('-') {
        folder.pop();
    }
    if folder.is_empty() {
        return FALLBACK_NAME.into();
    }
    if is_device_name(&folder) {
        folder = format!("{FALLBACK_NAME}-{folder}");
    }
    folder
}

/// Makes the first folder of `base`, `base-2`, `base-3`... that does not exist yet, and returns it.
/// The folder is created by a call that fails when it exists, so a name another process took in
/// the meantime moves on to the next number. `root` is private to the owner, so the folder is
/// unreachable to others until it is made private itself.
fn make_folder(root: &Path, base: &str) -> Result<PathBuf> {
    for number in 1..=SAME_NAME_FOLDERS {
        let name = if number == 1 {
            base.to_owned()
        } else {
            format!("{base}-{number}")
        };
        let path = root.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => {
                if let Err(error) = ensure_private_dir(&path) {
                    remove_unused(&path);
                    return Err(error.into());
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(errors::io(error)),
        }
    }
    Err(errors::invalid("There are too many projects of that name"))
}

/// Removes a folder this request made and nothing refers to: it is empty.
fn remove_unused(folder: &Path) {
    if let Err(error) = std::fs::remove_dir(folder) {
        soyeht_platform::log::failure("engine.project_folder_removal_failed", &error);
    }
}

/// Makes the folder of a new project. One folder is made: nothing is walked, so it stays under the
/// Core lock (ADR 0079).
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
    ensure_private_dir(&root)?;
    let folder = make_folder(&root, &folder_name(name))?;
    let path = match canonical_text(&folder) {
        Ok(path) => path,
        Err(error) => {
            remove_unused(&folder);
            return Err(error);
        }
    };
    let result = model::to_value(&ProjectFolderCreateResult { path })?;
    let made = core.mutate(connection, method::PROJECT_FOLDER_CREATE, params, |_| {
        Ok((result, Vec::new()))
    });
    match made {
        Ok(mutation) => Ok(mutation.result),
        Err(error) => {
            remove_unused(&folder);
            Err(error)
        }
    }
}

fn canonical_text(folder: &Path) -> Result<String> {
    canonicalize(folder)
        .map_err(errors::io)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| errors::internal("Project folder path must be UTF-8"))
}

#[cfg(test)]
mod tests;
