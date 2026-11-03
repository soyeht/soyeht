//! `service start` and a program that runs without its service (contract 13, section 3).

use super::{
    tests::{installed, launchd, mentions},
    *,
};
use crate::commands::fake::{Recorder, no, ok};

/// A service manager that has the units or agents registered and none of them running.
fn idle_services(system: System) -> Recorder {
    if system == System::Linux {
        Recorder::answering(|arguments| {
            if arguments.contains(&"is-active") {
                no()
            } else {
                ok("")
            }
        })
    } else {
        launchd(true, false)
    }
}

#[test]
fn start_leaves_alone_a_component_whose_program_runs_though_the_service_manager_says_idle() {
    // The window started the supervisor itself, and then the person registered the units: the
    // unit is idle but the program holds the lock.
    for (system, label) in [
        (System::Linux, "linux-start-held"),
        (System::Macos, "mac-start-held"),
    ] {
        let (_scratch, layout) =
            installed(system, label, &idle_services(system), &[Component::Ptyd]);
        let started =
            |recorder: &Recorder| mentions(recorder, " start ") || mentions(recorder, "bootstrap");
        let lock =
            soyeht_platform::fs::lock_singleton(&layout.data, Component::Ptyd.lock()).unwrap();
        let held = idle_services(system);
        Manager::new(&layout, &held)
            .start(&[Component::Ptyd])
            .unwrap();
        assert!(!started(&held), "{system:?}: {:?}", held.lines());
        // With nothing holding the lock the same call starts it.
        drop(lock);
        let free = idle_services(system);
        Manager::new(&layout, &free)
            .start(&[Component::Ptyd])
            .unwrap();
        assert!(started(&free), "{system:?}: {:?}", free.lines());
    }
}
