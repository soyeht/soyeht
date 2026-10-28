//! launchd: a user agent, loaded into the user's graphical domain (`gui/<uid>`), or into the
//! background domain (`user/<uid>`) of a session that has no screen, such as one over SSH.

use super::{Manager, path_text, remove_definition, write_definition};
use crate::{
    commands::Commands,
    error::{Error, Result},
    system::Component,
};

const LAUNCHCTL: &str = "launchctl";
/// What `launchctl print` says of a job that runs.
const RUNNING: &str = "state = running";

impl<C: Commands> Manager<'_, C> {
    /// The job's address, `<domain>/<label>`; the domain is the graphical one when the user has
    /// one.
    fn launchd_target(&self, component: Component) -> Result<String> {
        let graphical = format!("gui/{}", self.layout.user_id);
        let domain = if self.ask(LAUNCHCTL, &["print", &graphical])?.success {
            graphical
        } else {
            format!("user/{}", self.layout.user_id)
        };
        Ok(format!("{domain}/{}", self.name(component)))
    }

    /// The loaded job's own text, when the agent is loaded.
    fn launchd_job(&self, target: &str) -> Result<Option<String>> {
        let output = self.ask(LAUNCHCTL, &["print", target])?;
        Ok(output.success.then_some(output.stdout))
    }

    /// Loads the agent into the domain of `target`, which starts it: it runs at load.
    fn launchd_load(&self, component: Component, target: &str) -> Result<()> {
        let domain = target.rsplit_once('/').map_or(target, |(domain, _)| domain);
        let file = self.file(component);
        self.require(
            "launchctl bootstrap",
            LAUNCHCTL,
            &["bootstrap", domain, path_text(&file)?],
        )
    }

    pub(super) fn launchd_install(&self, component: Component, text: &str) -> Result<()> {
        write_definition(&self.file(component), text.as_bytes())?;
        let target = self.launchd_target(component)?;
        // A loaded agent keeps the definition it was loaded with. One that runs stays as it is:
        // booting it out would end the terminals of a supervisor. One that is loaded and not
        // running has nothing to lose, so it is loaded again with the new text.
        match self.launchd_job(&target)? {
            Some(job) if job.contains(RUNNING) => {}
            Some(_) => {
                self.require("launchctl bootout", LAUNCHCTL, &["bootout", &target])?;
                self.launchd_load(component, &target)?;
            }
            None => self.launchd_load(component, &target)?,
        }
        self.require("launchctl print", LAUNCHCTL, &["print", &target])
    }

    pub(super) fn launchd_start(&self, component: Component) -> Result<()> {
        if !self.file(component).is_file() {
            return Err(Error::NotRegistered);
        }
        let target = self.launchd_target(component)?;
        if self.launchd_job(&target)?.is_some() {
            self.require("launchctl kickstart", LAUNCHCTL, &["kickstart", &target])
        } else {
            self.launchd_load(component, &target)
        }
    }

    pub(super) fn launchd_stop(&self, component: Component) -> Result<()> {
        let target = self.launchd_target(component)?;
        if self.launchd_job(&target)?.is_some() {
            self.require("launchctl bootout", LAUNCHCTL, &["bootout", &target])?;
        }
        Ok(())
    }

    pub(super) fn launchd_uninstall(&self, component: Component) -> Result<()> {
        self.launchd_stop(component)?;
        remove_definition(&self.file(component))
    }

    /// `(registered, running)`: registered when the agent's file is in `LaunchAgents`, which
    /// launchd loads at every login; running when the loaded job says so.
    pub(super) fn launchd_status(&self, component: Component) -> Result<(bool, bool)> {
        let target = self.launchd_target(component)?;
        let job = self.launchd_job(&target)?;
        let running = job.is_some_and(|text| text.contains(RUNNING));
        Ok((self.file(component).is_file(), running))
    }
}
