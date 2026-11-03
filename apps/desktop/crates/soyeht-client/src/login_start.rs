//! Whether Soyeht starts when the person logs in, as the window asks and changes it (ADR 0136,
//! contract 13 sections 3 and 4).
//!
//! The window writes no service file and runs no service manager. It runs the `soyeht` program
//! that sits beside it, `soyeht service ...`, the one writer of a definition (ADR 0114), the way
//! the Mac app runs the one inside its bundle (ADR 0120): `status` says whether the profile's
//! services are registered, `install` registers them, `uninstall` removes the registration and
//! `start` asks the service manager to start what is registered.
//!
//! The state the window keeps is [`LoginStart`]: what the last question answered, whether a change
//! is under way and whether the person said "Not now" to being asked. The commands are behind
//! [`Ask`] so the decisions are tested without starting a program.

use crate::diagnostics::{Failure, ask_within};
use serde::Deserialize;
use soyeht_process::Limits;
use soyeht_protocol::client_policy::SERVICE_COMMAND_MS;
use std::time::Duration;

/// The setting's name and what it does, as the Preferences panel and the first-run question say
/// them (the Mac's question, "Keep Soyeht running in the background?", is the same one).
pub mod text {
    pub const TITLE: &str = "Start at login";
    pub const SETTING: &str = "Start Soyeht at login";
    pub const SUMMARY: &str = "Soyeht's engine keeps your terminals and agents running when this window is closed, \
         and starts when you log in.";
    pub const QUESTION: &str = "Keep Soyeht running in the background?";
    pub const TURN_ON: &str = "Turn on";
    pub const NOT_NOW: &str = "Not now";
    pub const LATER: &str = "You can turn it on later from Preferences.";
    pub const TURN_OFF_WARNING: &str =
        "Turning it off stops the engine once, which ends the terminals running now.";
    pub const WORKING: &str = "Working\u{2026}";
    pub const FAILED: &str = "Soyeht could not change this. The log has the reason.";
    pub const UNAVAILABLE: &str =
        "This copy has no soyeht program beside it, so it cannot start Soyeht at login.";
}

/// How long one `soyeht service` call may take. `systemctl enable --now` and the scheduled task's
/// start wait for a program to start, and the seam tries a busy service manager again.
const LIMITS: Limits = Limits::new(
    Duration::from_millis(SERVICE_COMMAND_MS),
    soyeht_protocol::limits::JSON_BYTES,
);

/// Runs `soyeht` with the arguments after the profile's own and returns what it printed.
pub(crate) type Ask<'a> = &'a dyn Fn(&[&str]) -> Result<String, Failure>;

/// The real [`Ask`]: the `soyeht` program beside this window.
pub(crate) fn real(arguments: &[&str]) -> Result<String, Failure> {
    ask_within(arguments, LIMITS)
}

#[derive(Deserialize)]
struct Report {
    services: Vec<Service>,
}

#[derive(Deserialize)]
struct Service {
    registered: bool,
}

/// What `soyeht service ...` printed, as whether any service of the profile is registered.
fn registered(printed: &str) -> Result<bool, Failure> {
    let report: Report = serde_json::from_str(printed).map_err(|_| Failure::Unreadable)?;
    Ok(report.services.iter().any(|service| service.registered))
}

/// Whether the profile's services are registered to start at login.
pub(crate) fn is_registered(ask: Ask<'_>) -> Result<bool, Failure> {
    registered(&ask(&["service", "status"])?)
}

/// Asks the service manager to start what is registered, the supervisor first. An error means the
/// start was not accepted: a service that is not registered, a program that is gone, a service
/// manager that refuses.
pub(crate) fn start(ask: Ask<'_>) -> Result<(), Failure> {
    ask(&["service", "start"]).map(drop)
}

/// Registers the services and starts those that do not run. A program the window started stays
/// as it is (contract 13, section 3).
pub(crate) fn turn_on(ask: Ask<'_>) -> Result<(), Failure> {
    if registered(&ask(&["service", "install"])?)? {
        Ok(())
    } else {
        Err(Failure::Run("not_registered".to_owned()))
    }
}

/// Unregisters the services. It stops them first (`service uninstall`), so the terminals end.
pub(crate) fn turn_off(ask: Ask<'_>) -> Result<(), Failure> {
    if registered(&ask(&["service", "uninstall"])?)? {
        Err(Failure::Run("still_registered".to_owned()))
    } else {
        Ok(())
    }
}

/// Whether the services are registered, asked of the `soyeht` program beside this window. Blocks
/// until it answers: run it off the window's thread.
pub fn status() -> Result<bool, Failure> {
    is_registered(&real)
}

/// [`turn_on`] with the real program. Blocks.
pub fn register() -> Result<(), Failure> {
    turn_on(&real)
}

/// [`turn_off`] with the real program. Blocks.
pub fn unregister() -> Result<(), Failure> {
    turn_off(&real)
}

/// Where the setting is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    /// Not known yet, or this copy cannot register (no `soyeht` beside it, an engine chosen by the
    /// environment).
    #[default]
    Unavailable,
    /// The first question is out.
    Checking,
    Off,
    On,
    /// A change to this state is under way.
    Changing(bool),
    /// The last change failed, and the setting is as it was.
    Failed(bool, Failure),
}

/// The setting "Start Soyeht at login", with the question the window asks once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoginStart {
    phase: Phase,
    declined: bool,
}

impl LoginStart {
    /// A window that has not asked yet; `declined` is the person's earlier "Not now".
    pub fn new(declined: bool) -> Self {
        Self {
            phase: Phase::Unavailable,
            declined,
        }
    }

    /// Starts the first question, unless one was asked: false means do nothing.
    pub fn begin_check(&mut self) -> bool {
        if self.phase != Phase::Unavailable {
            return false;
        }
        self.phase = Phase::Checking;
        true
    }

    /// The question was answered. A copy with no `soyeht` program beside it cannot register and
    /// asks nothing; any other failure leaves the setting off, with the reason said.
    pub fn checked(&mut self, answer: Result<bool, Failure>) {
        self.phase = match answer {
            Ok(true) => Phase::On,
            Ok(false) => Phase::Off,
            Err(Failure::Unavailable) => Phase::Unavailable,
            Err(failure) => Phase::Failed(false, failure),
        };
    }

    /// The person turns the setting on or off. False when a change is already under way or the
    /// setting has no answer yet, and then nothing should run.
    pub fn begin_change(&mut self, on: bool) -> bool {
        if matches!(
            self.phase,
            Phase::Unavailable | Phase::Checking | Phase::Changing(_)
        ) || self.is_on() == on && !matches!(self.phase, Phase::Failed(..))
        {
            return false;
        }
        self.phase = Phase::Changing(on);
        true
    }

    /// The change to `on` ended. A failure leaves the setting as it was before.
    pub fn changed(&mut self, on: bool, result: Result<(), Failure>) {
        self.phase = match result {
            Ok(()) => {
                // Turning it on answers the question yes, and turning it off answers it no: the
                // window does not ask a person who just chose.
                self.declined = !on;
                if on { Phase::On } else { Phase::Off }
            }
            Err(failure) => Phase::Failed(!on, failure),
        };
    }

    /// The person said "Not now": the window does not ask again.
    pub fn decline(&mut self) {
        self.declined = true;
    }

    /// Whether the window asks the first-run question now.
    pub fn should_ask(&self) -> bool {
        self.phase == Phase::Off && !self.declined
    }

    /// Whether the person said "Not now" and has not turned it on since.
    pub fn declined(&self) -> bool {
        self.declined
    }

    /// What the switch shows: on while registered, and while a change to on is under way.
    pub fn is_on(&self) -> bool {
        match self.phase {
            Phase::On => true,
            Phase::Changing(on) => on,
            Phase::Failed(was_on, _) => was_on,
            Phase::Unavailable | Phase::Checking | Phase::Off => false,
        }
    }

    /// Whether the switch answers a click now.
    pub fn can_change(&self) -> bool {
        matches!(self.phase, Phase::On | Phase::Off | Phase::Failed(..))
    }

    /// Whether the setting is in a state that has something to show.
    pub fn is_available(&self) -> bool {
        self.phase != Phase::Unavailable
    }

    /// The sentence under the switch, if the setting has one to say, and whether it reports a
    /// problem.
    pub fn message(&self) -> Option<(&'static str, bool)> {
        match self.phase {
            Phase::Changing(_) | Phase::Checking => Some((text::WORKING, false)),
            Phase::Failed(..) => Some((text::FAILED, true)),
            Phase::Unavailable | Phase::Off | Phase::On => None,
        }
    }
}

#[cfg(test)]
mod tests;
