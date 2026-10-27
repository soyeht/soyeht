//! Who has a web pane, the person or an agent: the Mac's `WebControlHandoff`. A person's input in
//! the pane stops an agent from driving it until the person hands it back.

use super::HostError;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub struct Handoff {
    person: AtomicBool,
}

impl Handoff {
    /// The person pressed, scrolled or typed in the page.
    pub fn note_person(&self) {
        self.person.store(true, Ordering::Release);
    }

    /// The person gives the pane back to the agents.
    pub fn hand_back(&self) {
        self.person.store(false, Ordering::Release);
    }

    pub fn person_has_control(&self) -> bool {
        self.person.load(Ordering::Acquire)
    }

    /// An agent may start a command: refused while the person has the pane.
    pub fn agent_may_drive(&self) -> Result<(), HostError> {
        if self.person_has_control() {
            return Err(Self::took_over());
        }
        Ok(())
    }

    /// The error of a command that finds the person has the pane.
    pub fn took_over() -> HostError {
        HostError::HumanTookOver("A person has control of this web pane. Wait for hand-back.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_who_touches_the_page_stops_agents_until_the_pane_is_handed_back() {
        let handoff = Handoff::default();
        assert!(handoff.agent_may_drive().is_ok());

        handoff.note_person();
        assert_eq!(
            handoff.agent_may_drive().unwrap_err(),
            HostError::HumanTookOver("A person has control of this web pane. Wait for hand-back.")
        );
        handoff.hand_back();
        assert!(handoff.agent_may_drive().is_ok());
    }
}
