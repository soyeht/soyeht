//! systemd: a user unit in `~/.config/systemd/user`, driven with `systemctl --user`.
//!
//! `enable` links the unit into the user's `default.target`, which the user's manager starts at
//! login (and, with lingering enabled by the person, at boot).

use super::{Manager, remove_definition, write_definition};
use crate::{
    commands::Commands,
    error::{Error, Result},
    system::Component,
};

const SYSTEMCTL: &str = "systemctl";

impl<C: Commands> Manager<'_, C> {
    /// A `systemctl --user` command that must succeed.
    fn systemd_do(&self, step: &'static str, arguments: &[&str]) -> Result<()> {
        let mut all = vec!["--user"];
        all.extend_from_slice(arguments);
        self.require(step, SYSTEMCTL, &all)
    }

    /// Whether a `systemctl --user` question about the unit has the answer yes.
    fn systemd_says(&self, question: &str, component: Component) -> Result<bool> {
        let unit = self.name(component);
        Ok(self.ask(SYSTEMCTL, &["--user", question, unit])?.success)
    }

    pub(super) fn systemd_install(&self, component: Component, text: &str) -> Result<()> {
        write_definition(&self.file(component), text.as_bytes())?;
        self.systemd_do("systemctl daemon-reload", &["daemon-reload"])?;
        // `--now` starts a unit that is not running and leaves one that is.
        let unit = self.name(component);
        self.systemd_do("systemctl enable", &["enable", "--now", unit])
    }

    pub(super) fn systemd_start(&self, component: Component) -> Result<()> {
        if !self.file(component).is_file() {
            return Err(Error::NotRegistered);
        }
        self.systemd_do("systemctl start", &["start", self.name(component)])
    }

    pub(super) fn systemd_stop(&self, component: Component) -> Result<()> {
        if !self.file(component).is_file() {
            return Ok(());
        }
        self.systemd_do("systemctl stop", &["stop", self.name(component)])
    }

    pub(super) fn systemd_uninstall(&self, component: Component) -> Result<()> {
        let file = self.file(component);
        if file.is_file() {
            let unit = self.name(component);
            self.systemd_do("systemctl disable", &["disable", "--now", unit])?;
        }
        remove_definition(&file)?;
        self.systemd_do("systemctl daemon-reload", &["daemon-reload"])
    }

    /// `(registered, running)`: registered when the unit's file is in place and enabled.
    pub(super) fn systemd_status(&self, component: Component) -> Result<(bool, bool)> {
        let present = self.file(component).is_file();
        Ok((
            present && self.systemd_says("is-enabled", component)?,
            present && self.systemd_says("is-active", component)?,
        ))
    }
}
