//! launchd: a user agent, loaded into the user's graphical domain (`gui/<uid>`), or into the
//! background domain (`user/<uid>`) of a session that has no screen, such as one over SSH.

use super::{Manager, path_text, remove_definition, write_definition};
use crate::{
    commands::Commands,
    error::{Error, Result},
    policy,
    render::xml_unescape,
    system::Component,
};

const LAUNCHCTL: &str = "launchctl";
/// What `launchctl print` says of a job that runs.
const RUNNING: &str = "state = running";
/// What `launchctl bootstrap` says while launchd is still removing a job that was booted out:
/// "5: Input/output error" or "37: Operation already in progress". Nothing else is retried.
const TRANSIENT: [&str; 2] = ["5: Input/output error", "37: Operation already in progress"];

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

    /// Loads the agent into the domain of `target`, which starts it: it runs at load. Right after
    /// a `bootout` launchd can still be tearing the job down and refuses the new one for a
    /// moment, so a refusal of that kind is tried again after a short wait, a bounded number of
    /// times. Any other failure is returned at once.
    fn launchd_load(&self, component: Component, target: &str) -> Result<()> {
        let domain = target.rsplit_once('/').map_or(target, |(domain, _)| domain);
        let file = self.file(component);
        let arguments = ["bootstrap", domain, path_text(&file)?];
        let mut waits = policy::LAUNCHD_RELOAD_BACKOFF.into_iter();
        loop {
            let result = self.require("launchctl bootstrap", LAUNCHCTL, &arguments);
            let transient = matches!(
                &result,
                Err(Error::Failed { detail, .. }) if TRANSIENT.iter().any(|text| detail.contains(text))
            );
            match waits.next() {
                Some(wait) if transient => self.commands.pause(wait),
                _ => return result,
            }
        }
    }

    pub(super) fn launchd_install(
        &self,
        component: Component,
        text: &str,
        start: bool,
    ) -> Result<()> {
        write_definition(&self.file(component), text.as_bytes())?;
        if !start {
            // The agent's file in `LaunchAgents` is the registration: launchd loads it at the
            // next login. Loading it now would start a second copy of a program that runs.
            return Ok(());
        }
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
        // An agent whose program is gone (the app was dragged to the Trash) is kept from
        // starting by its `PathState`, so loading it would run nothing.
        self.require_startable(component, agent_program)?;
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

/// The program a property list starts: the first string of its `ProgramArguments`.
fn agent_program(plist: &str) -> Option<String> {
    let (_, after) = plist.split_once("<key>ProgramArguments</key>")?;
    let (_, after) = after.split_once("<string>")?;
    let (program, _) = after.split_once("</string>")?;
    Some(xml_unescape(program))
}

#[cfg(test)]
mod tests {
    use super::agent_program;
    use crate::{definition::sample, render::render, system::System};

    #[test]
    fn the_program_is_read_back_from_the_property_list_as_it_was_written() {
        let mut definition = sample("com.soyeht.next.test.engine");
        assert_eq!(
            agent_program(&render(System::Macos, &definition).unwrap()).as_deref(),
            Some("/opt/next/soyeht-engine")
        );
        definition.program = "/Apps/A & B <next>/soyeht-engine".into();
        assert_eq!(
            agent_program(&render(System::Macos, &definition).unwrap()).as_deref(),
            Some("/Apps/A & B <next>/soyeht-engine")
        );
        assert_eq!(agent_program("<plist></plist>"), None);
    }
}
