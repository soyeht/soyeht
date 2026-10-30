//! Task Scheduler: a per-user task that runs at logon, driven with `schtasks`.
//!
//! `schtasks /Create /XML` takes the whole definition from a file, so the task has exactly the
//! settings the renderer wrote. The copy of that file stays in the data directory as the record
//! of what was registered. Whether a task runs is asked of PowerShell, since the column
//! `schtasks` prints is translated to the language of the system.

use super::{Manager, path_text, remove_definition, write_definition};
use crate::{
    commands::Commands,
    error::{Error, Result},
    render::encode_task_file,
    system::Component,
};
use soyeht_platform::fs::ensure_private_dir;

const SCHTASKS: &str = "schtasks";
const POWERSHELL: &str = "powershell.exe";

impl<C: Commands> Manager<'_, C> {
    fn task_exists(&self, component: Component) -> Result<bool> {
        let name = self.name(component);
        Ok(self.ask(SCHTASKS, &["/Query", "/TN", name])?.success)
    }

    fn task_runs(&self, component: Component) -> Result<bool> {
        // The names of a profile are letters, digits and dots (the generator checks), so they
        // need no quoting inside the single quotes.
        let script = format!(
            "if ((Get-ScheduledTask -TaskName '{}').State -eq 'Running') {{ 'running' }} else {{ 'stopped' }}",
            self.name(component)
        );
        let arguments = ["-NoProfile", "-NonInteractive", "-Command", &script];
        let output = self.ask(POWERSHELL, &arguments)?;
        Ok(output.success && output.stdout.trim() == "running")
    }

    /// Runs the task now.
    fn task_run(&self, component: Component) -> Result<()> {
        let name = self.name(component);
        self.require("schtasks run", SCHTASKS, &["/Run", "/TN", name])
    }

    pub(super) fn task_install(&self, component: Component, text: &str, start: bool) -> Result<()> {
        let file = self.file(component);
        if let Some(folder) = file.parent() {
            ensure_private_dir(folder)?;
        }
        write_definition(&file, &encode_task_file(text))?;
        // `/F` replaces a task of the same name, and leaves its running instance alone.
        let name = self.name(component);
        let arguments = ["/Create", "/TN", name, "/XML", path_text(&file)?, "/F"];
        self.require("schtasks create", SCHTASKS, &arguments)?;
        if !start || self.task_runs(component)? {
            Ok(())
        } else {
            self.task_run(component)
        }
    }

    pub(super) fn task_start(&self, component: Component) -> Result<()> {
        if self.task_exists(component)? {
            self.task_run(component)
        } else {
            Err(Error::NotRegistered)
        }
    }

    pub(super) fn task_stop(&self, component: Component) -> Result<()> {
        if self.task_exists(component)? && self.task_runs(component)? {
            let name = self.name(component);
            self.require("schtasks end", SCHTASKS, &["/End", "/TN", name])?;
        }
        Ok(())
    }

    pub(super) fn task_uninstall(&self, component: Component) -> Result<()> {
        if self.task_exists(component)? {
            self.task_stop(component)?;
            let name = self.name(component);
            self.require("schtasks delete", SCHTASKS, &["/Delete", "/TN", name, "/F"])?;
        }
        remove_definition(&self.file(component))
    }

    /// `(registered, running)`: registered when the scheduler has the task.
    pub(super) fn task_status(&self, component: Component) -> Result<(bool, bool)> {
        if self.task_exists(component)? {
            Ok((true, self.task_runs(component)?))
        } else {
            Ok((false, false))
        }
    }
}
