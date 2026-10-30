//! Registering, starting, stopping and removing the service, and asking about it.
//!
//! A [`Manager`] works for one [`Layout`]. Each system has its own file in this folder with the
//! sequence of its tool's commands; the manager only decides which components and in what
//! order. Everything that reaches the system goes through [`Commands`], and the files it writes
//! go to the places the layout names, so a test points both at a temporary folder and a fake.

mod linux;
mod macos;
mod windows;

use crate::{
    commands::{CommandOutput, Commands, require},
    error::{Error, Result},
    layout::Layout,
    render::{encode_task_file, render},
    system::{Component, System},
};
use soyeht_platform::fs::{
    OpenOptionsPrivateExt as _, WriteMode, ensure_private_dir, set_private_file, write_atomic,
};
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

/// What the system says about one component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub component: Component,
    /// The name the system knows it by.
    pub name: &'static str,
    /// The system will start it at login: its definition is in place.
    pub registered: bool,
    /// It is running now.
    pub running: bool,
}

/// The service of one profile on one system.
pub struct Manager<'a, C: Commands> {
    layout: &'a Layout,
    commands: &'a C,
}

/// What the system files of this folder ask of a manager.
impl<C: Commands> Manager<'_, C> {
    /// Before a start: fails with `NotRegistered` when there is no definition, and with
    /// `ProgramMissing` when the program the definition starts (as `read_program` finds it in the
    /// text) is no longer there. A start that would run nothing must not say it worked: systemd
    /// skips such a unit and launchd does not run such an agent, and both report success.
    fn require_startable(
        &self,
        component: Component,
        read_program: fn(&str) -> Option<String>,
    ) -> Result<()> {
        let file = self.file(component);
        if !file.is_file() {
            return Err(Error::NotRegistered);
        }
        if let Some(program) = read_program(&fs::read_to_string(&file)?)
            && !Path::new(&program).is_file()
        {
            return Err(Error::ProgramMissing(program));
        }
        Ok(())
    }

    /// The name the system knows `component` by.
    fn name(&self, component: Component) -> &'static str {
        self.layout.name(component)
    }

    /// The file that holds the definition of `component`.
    fn file(&self, component: Component) -> PathBuf {
        self.layout.definition_file(component)
    }

    /// Runs a command and returns what it printed, whether or not it succeeded.
    fn ask(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput> {
        self.commands.run(program, arguments)
    }

    /// Runs a command that must succeed; `step` names it in the error.
    fn require(&self, step: &'static str, program: &str, arguments: &[&str]) -> Result<()> {
        require(self.commands, step, program, arguments).map(drop)
    }
}

impl<'a, C: Commands> Manager<'a, C> {
    pub fn new(layout: &'a Layout, commands: &'a C) -> Self {
        Self { layout, commands }
    }

    /// Writes the definition of each component in `components` (programs found in `programs`),
    /// registers it with the system and starts it, the supervisor first. A definition that is
    /// already registered and running is rewritten and left running: the new text applies when
    /// the system next loads it (after `stop` and `start`, or at the next login), so a person's
    /// terminals are not ended by an installer that only repairs the registration. A launchd
    /// agent that is loaded but not running is loaded again, since nothing is lost. Nothing is
    /// written when a definition is refused or a program is missing.
    pub fn install(&self, programs: &Path, components: &[Component]) -> Result<()> {
        let mut texts = Vec::new();
        for component in ordered(components) {
            let definition = self.layout.definition(component, programs)?;
            // A service whose program is missing would be restarted, and fail, for ever.
            if let Some(missing) = definition
                .files
                .iter()
                .find(|file| !Path::new(file).is_file())
            {
                return Err(Error::ProgramMissing(missing.clone()));
            }
            texts.push((component, render(self.layout.system, &definition)?));
        }
        self.prepare_folders(texts.iter().map(|(component, _)| *component))?;
        for (component, text) in texts {
            match self.layout.system {
                System::Macos => self.launchd_install(component, &text)?,
                System::Linux => self.systemd_install(component, &text)?,
                System::Windows => self.task_install(component, &text)?,
            }
        }
        Ok(())
    }

    /// Stops each component and removes its registration and definition, the engine first. The
    /// data, the database and the logs stay.
    pub fn uninstall(&self, components: &[Component]) -> Result<()> {
        for component in ordered(components).into_iter().rev() {
            match self.layout.system {
                System::Macos => self.launchd_uninstall(component)?,
                System::Linux => self.systemd_uninstall(component)?,
                System::Windows => self.task_uninstall(component)?,
            }
        }
        Ok(())
    }

    /// Starts each registered component, the supervisor first. A component that already runs is
    /// left alone.
    pub fn start(&self, components: &[Component]) -> Result<()> {
        for component in ordered(components) {
            if self.status(component)?.running {
                continue;
            }
            match self.layout.system {
                System::Macos => self.launchd_start(component)?,
                System::Linux => self.systemd_start(component)?,
                System::Windows => self.task_start(component)?,
            }
        }
        Ok(())
    }

    /// Stops each component that runs, the engine first, and leaves it registered where the
    /// system allows: Linux and Windows start it again at the next login, and on the Mac it is
    /// loaded again at the next login too.
    pub fn stop(&self, components: &[Component]) -> Result<()> {
        for component in ordered(components).into_iter().rev() {
            match self.layout.system {
                System::Macos => self.launchd_stop(component)?,
                System::Linux => self.systemd_stop(component)?,
                System::Windows => self.task_stop(component)?,
            }
        }
        Ok(())
    }

    /// Whether the definition on disk is the one `install` would write for programs found in
    /// `programs`: the same bytes. A caller that can be moved (the Mac app) asks this before it
    /// installs, so a registration that is right is left alone, and one that a person stopped
    /// with `stop` is not started again.
    pub fn is_current(&self, programs: &Path, component: Component) -> Result<bool> {
        let definition = self.layout.definition(component, programs)?;
        let wanted = file_bytes(
            self.layout.system,
            &render(self.layout.system, &definition)?,
        );
        match fs::read(self.file(component)) {
            Ok(found) => Ok(found == wanted),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn status(&self, component: Component) -> Result<Status> {
        let (registered, running) = match self.layout.system {
            System::Macos => self.launchd_status(component)?,
            System::Linux => self.systemd_status(component)?,
            System::Windows => self.task_status(component)?,
        };
        Ok(Status {
            component,
            name: self.layout.name(component),
            registered,
            running,
        })
    }

    /// Makes the data and log folders owner-only and the log files present, so the system's
    /// manager (which will not make a folder) has somewhere to append to.
    fn prepare_folders(&self, components: impl Iterator<Item = Component>) -> Result<()> {
        ensure_private_dir(&self.layout.data)?;
        ensure_private_dir(&soyeht_platform::process::detached::logs_directory(
            &self.layout.data,
        ))?;
        for component in components {
            let log = self.layout.log(component);
            OpenOptions::new()
                .create(true)
                .append(true)
                .owner_only()
                .open_no_follow(&log)?;
            set_private_file(&log)?;
        }
        Ok(())
    }
}

/// The components in the order they start, once each.
fn ordered(components: &[Component]) -> Vec<Component> {
    Component::START_ORDER
        .into_iter()
        .filter(|component| components.contains(component))
        .collect()
}

/// The bytes of a definition file: a Task Scheduler file is UTF-16, the others are text.
fn file_bytes(system: System, text: &str) -> Vec<u8> {
    match system {
        System::Windows => encode_task_file(text),
        System::Macos | System::Linux => text.as_bytes().to_vec(),
    }
}

/// Replaces the file at `path`, making its folder when missing; a link at `path` is replaced,
/// not written through. A regular file that already holds exactly `bytes` is left alone, so a
/// repair that changes nothing leaves no new modification time and no new background item for
/// the system to announce.
pub(crate) fn write_definition(path: &Path, bytes: &[u8]) -> Result<()> {
    let regular = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file());
    if regular && fs::read(path).is_ok_and(|found| found == bytes) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(path, bytes, WriteMode::OwnerOnly)?;
    Ok(())
}

/// Removes a file the seam wrote; one that is already gone is what was wanted.
pub(crate) fn remove_definition(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
        // A missing file is the state the caller asked for.
        _ => Ok(()),
    }
}

/// `path` as the text a command line takes.
pub(crate) fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or(Error::Unrepresentable("path"))
}

#[cfg(test)]
mod current_tests;
#[cfg(test)]
mod program_tests;
#[cfg(test)]
mod tests;
