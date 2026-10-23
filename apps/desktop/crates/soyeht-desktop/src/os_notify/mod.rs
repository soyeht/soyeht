//! The system's notification: a banner the person sees while the window is behind other windows or
//! minimized, and a click on it.
//!
//! Linux speaks the freedesktop `Notify` call over D-Bus and Windows shows a toast; both come from
//! one crate (ADR 0103). Every other system returns [`Error::Unsupported`], so the caller has one
//! shape on every system. [`show`] and [`Shown::wait`] block (a D-Bus round trip, then the wait for
//! the person), so they belong on a thread of their own.

use soyeht_platform::log::Failure;

#[cfg_attr(any(target_os = "linux", windows), path = "supported.rs")]
#[cfg_attr(not(any(target_os = "linux", windows)), path = "unsupported.rs")]
mod imp;

/// The name the system shows for this application, and the desktop entry it matches.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
const APP_NAME: &str = "Soyeht";

/// What a notification says. A system without notifications never reads it.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
pub(crate) struct Toast<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    /// This system has no notification the client can show.
    #[cfg_attr(any(target_os = "linux", windows), allow(dead_code))]
    Unsupported,
    /// The system has them but did not take this one: no notification service runs, or the
    /// session has no bus.
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    Unavailable,
}

impl Failure for Error {
    fn failure_kind(&self) -> String {
        match self {
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
        }
        .to_owned()
    }
}

/// How a shown notification ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The person clicked it (its body, or its default action).
    Activated,
    /// It went away: dismissed, expired, or the system stopped tracking it.
    Closed,
}

/// A notification the system has taken.
pub(crate) struct Shown(imp::Shown);

/// Shows `toast`. Blocks until the system has taken it.
pub(crate) fn show(toast: &Toast) -> Result<Shown, Error> {
    imp::show(toast).map(Shown)
}

impl Shown {
    /// Blocks until the person clicks the notification or it goes away.
    pub(crate) fn wait(self) -> Outcome {
        self.0.wait()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_logs_its_kind_and_never_a_message() {
        assert_eq!(Error::Unsupported.failure_kind(), "unsupported");
        assert_eq!(Error::Unavailable.failure_kind(), "unavailable");
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    #[test]
    fn a_system_without_notifications_says_so() {
        let toast = Toast {
            title: "title",
            body: "body",
        };
        assert_eq!(show(&toast).err(), Some(Error::Unsupported));
    }
}
