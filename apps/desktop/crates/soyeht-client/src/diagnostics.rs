//! "Copy diagnostics" (ADR 0118, contract 14 section 4): the redacted report of `soyeht
//! diagnostics`, on the clipboard.
//!
//! The report is built by the `soyeht` program that sits beside this executable, and only there:
//! the window runs it for the profile and data directory it resolved and takes what it prints as
//! it is, so it adds no fact and no redaction rule of its own, and it works with no engine.

use crate::{autostart::program_file, profile};
use soyeht_platform::log;
use soyeht_process::Limits;
use soyeht_protocol::client_policy::{
    CAPS_DIAGNOSTICS_REPORT_BYTES, DIAGNOSTICS_COPIED, DIAGNOSTICS_FAILED, DIAGNOSTICS_MS,
    DIAGNOSTICS_UNAVAILABLE, DIAGNOSTICS_WORKING,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

const PROGRAM: &str = "soyeht";

/// Why there is no report to copy. [`log::Failure::failure_kind`] is what the log keeps; the
/// message the program printed can carry a path, so it is neither logged nor shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The `soyeht` program is not in the folder of this one.
    Unavailable,
    /// The profile or its data directory could not be resolved.
    Profile,
    /// The program did not run or finish, or ended with an error: the runner's kind or the exit
    /// status.
    Run(String),
    /// The program printed something that is not text.
    NotText,
}

impl log::Failure for Failure {
    fn failure_kind(&self) -> String {
        match self {
            Self::Unavailable => "unavailable".to_owned(),
            Self::Profile => "profile".to_owned(),
            Self::Run(kind) => format!("run:{kind}"),
            Self::NotText => "not_text".to_owned(),
        }
    }
}

/// The report for the profile this window runs as, from the `soyeht` program beside this
/// executable. Blocks until the program ends: run it off the window's thread.
pub fn collect() -> Result<String, Failure> {
    // A system that cannot say where this program is has no folder to look in.
    let executable = std::env::current_exe().map_err(|_| Failure::Unavailable)?;
    let profile = profile::selected().map_err(|_| Failure::Profile)?;
    // The profile's own data directory, also when the window was pointed at another engine with
    // `SOYEHT_ENGINE_SOCKET` (contract 14, section 4).
    let directory = profile::data_directory_of_profile().map_err(|_| Failure::Profile)?;
    collect_with(&executable, profile.name, &directory)
}

/// The report of `profile`, whose data is in `directory`, from the `soyeht` program that sits in
/// the folder of `executable`.
fn collect_with(executable: &Path, profile: &str, directory: &Path) -> Result<String, Failure> {
    run(&program_beside(executable)?, &arguments(profile, directory))
}

/// The `soyeht` program in the folder of `executable`.
fn program_beside(executable: &Path) -> Result<PathBuf, Failure> {
    executable
        .parent()
        .map(|programs| program_file(programs, PROGRAM))
        .filter(|program| program.is_file())
        .ok_or(Failure::Unavailable)
}

/// The command line that asks for the report of a profile, the one the Mac app builds too.
fn arguments(profile: &str, directory: &Path) -> Vec<OsString> {
    vec![
        "--profile".into(),
        profile.into(),
        "--data-dir".into(),
        directory.as_os_str().to_owned(),
        "diagnostics".into(),
    ]
}

fn run(program: &Path, arguments: &[OsString]) -> Result<String, Failure> {
    let mut command = std::process::Command::new(program);
    command.args(arguments);
    let output = soyeht_process::run(
        command,
        Limits::new(
            Duration::from_millis(DIAGNOSTICS_MS),
            CAPS_DIAGNOSTICS_REPORT_BYTES,
        ),
    )
    .map_err(|error| Failure::Run(log::Failure::failure_kind(&error)))?;
    if !output.success() {
        return Err(Failure::Run(output.status.code().map_or_else(
            || "signal".to_owned(),
            |code| format!("status:{code}"),
        )));
    }
    String::from_utf8(output.stdout).map_err(|_| Failure::NotText)
}

/// Where the action is, for the window to draw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Working,
    /// The report is on the clipboard.
    Copied,
    Failed(Failure),
}

impl Phase {
    /// Starts a collection, unless one is running: false means do nothing.
    pub fn begin(&mut self) -> bool {
        if *self == Self::Working {
            return false;
        }
        *self = Self::Working;
        true
    }

    /// Records how the collection ended, the clipboard write included.
    pub fn finish(&mut self, result: Result<(), Failure>) {
        *self = match result {
            Ok(()) => Self::Copied,
            Err(failure) => Self::Failed(failure),
        };
    }

    pub fn working(&self) -> bool {
        *self == Self::Working
    }

    /// The sentence under the button, if the action has one to say (contract 14, section 4).
    pub fn message(&self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Working => Some(DIAGNOSTICS_WORKING),
            Self::Copied => Some(DIAGNOSTICS_COPIED),
            Self::Failed(Failure::Unavailable) => Some(DIAGNOSTICS_UNAVAILABLE),
            Self::Failed(_) => Some(DIAGNOSTICS_FAILED),
        }
    }

    /// Whether the sentence reports a problem.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

#[cfg(test)]
mod tests;
