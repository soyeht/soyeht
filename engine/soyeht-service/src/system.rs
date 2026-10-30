//! The systems a service can be registered on, and the two programs that make up the service.

use soyeht_protocol::profile::Profile;

/// An operating system with a service manager the seam drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    /// launchd, through a user agent in `~/Library/LaunchAgents`.
    Macos,
    /// systemd, through a user unit (`systemctl --user`).
    Linux,
    /// Task Scheduler, through a per-user task that runs at logon.
    Windows,
}

impl System {
    /// The system this program was built for.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// The name used on a command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }

    /// The system a command line names.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Macos, Self::Linux, Self::Windows]
            .into_iter()
            .find(|system| system.name() == name)
    }

    /// The suffix of an executable's file name.
    pub(crate) const fn executable_suffix(self) -> &'static str {
        match self {
            Self::Windows => ".exe",
            Self::Macos | Self::Linux => "",
        }
    }
}

/// One of the two programs a service keeps running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    /// `soyeht-ptyd`, the process supervisor that holds the pseudoterminals.
    Ptyd,
    /// `soyeht-engine`, which owns the state and serves the clients.
    Engine,
}

impl Component {
    /// Both components, in the order they start: the supervisor first. Stopping goes the other
    /// way round.
    pub const START_ORDER: [Self; 2] = [Self::Ptyd, Self::Engine];

    /// The name used on a command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ptyd => "ptyd",
            Self::Engine => "engine",
        }
    }

    /// The component a command line names.
    pub fn parse(name: &str) -> Option<Self> {
        Self::START_ORDER
            .into_iter()
            .find(|component| component.name() == name)
    }

    /// The file name of the program, without a suffix.
    pub(crate) const fn program(self) -> &'static str {
        match self {
            Self::Ptyd => "soyeht-ptyd",
            Self::Engine => "soyeht-engine",
        }
    }

    /// The singleton lock the program takes in the data directory while it runs.
    pub(crate) const fn lock(self) -> &'static str {
        match self {
            Self::Ptyd => soyeht_platform::fs::PTYD_LOCK,
            Self::Engine => soyeht_platform::fs::ENGINE_LOCK,
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::Ptyd => "Soyeht terminal supervisor",
            Self::Engine => "Soyeht engine",
        }
    }
}

/// The name the system's service manager knows `component` of `profile` by: the launchd label,
/// the systemd unit file name or the Task Scheduler task name.
pub(crate) const fn service_name(
    profile: &Profile,
    system: System,
    component: Component,
) -> &'static str {
    match (system, component) {
        (System::Macos, Component::Engine) => profile.engine_launchd_label,
        (System::Macos, Component::Ptyd) => profile.ptyd_launchd_label,
        (System::Linux, Component::Engine) => profile.engine_systemd_unit,
        (System::Linux, Component::Ptyd) => profile.ptyd_systemd_unit,
        (System::Windows, Component::Engine) => profile.engine_windows_task,
        (System::Windows, Component::Ptyd) => profile.ptyd_windows_task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::profile::{DEV, PROD, PROFILES};

    #[test]
    fn names_round_trip_through_the_command_line() {
        for system in [System::Macos, System::Linux, System::Windows] {
            assert_eq!(System::parse(system.name()), Some(system));
        }
        for component in Component::START_ORDER {
            assert_eq!(Component::parse(component.name()), Some(component));
        }
        assert_eq!(System::parse("beos"), None);
        assert_eq!(Component::parse("Engine"), None);
    }

    #[test]
    fn every_system_gets_the_names_of_its_profile() {
        assert_eq!(
            service_name(&PROD, System::Macos, Component::Engine),
            PROD.engine_launchd_label
        );
        assert_eq!(
            service_name(&PROD, System::Linux, Component::Ptyd),
            PROD.ptyd_systemd_unit
        );
        assert_eq!(
            service_name(&DEV, System::Windows, Component::Engine),
            DEV.engine_windows_task
        );
    }

    #[test]
    fn no_two_services_of_any_profile_share_a_name_on_one_system() {
        for system in [System::Macos, System::Linux, System::Windows] {
            let mut seen = Vec::new();
            for profile in PROFILES {
                for component in Component::START_ORDER {
                    let name = service_name(profile, system, component);
                    assert!(!seen.contains(&name), "{name} is used twice");
                    seen.push(name);
                }
            }
        }
    }
}
