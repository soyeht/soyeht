//! `service start` and a program that runs without its service (contract 13, section 3).

use super::{
    tests::{BOTH, installed, launchd, mentions, programs},
    *,
};
use crate::{
    commands::fake::{Recorder, no, ok},
    layout::test_layout,
    testing::Scratch,
};

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

#[test]
fn an_engine_only_start_on_linux_starts_the_supervisor_first_when_it_is_registered_and_free() {
    let system = System::Linux;
    let (_scratch, layout) = installed(system, "linux-engine-only", &idle_services(system), &BOTH);
    let starts = |recorder: &Recorder| -> Vec<String> {
        recorder
            .lines()
            .into_iter()
            .filter(|line| line.contains(" start "))
            .collect()
    };
    let free = idle_services(system);
    Manager::new(&layout, &free)
        .start(&[Component::Engine])
        .unwrap();
    let (ptyd, engine) = (
        layout.profile.ptyd_systemd_unit,
        layout.profile.engine_systemd_unit,
    );
    assert_eq!(
        starts(&free),
        [
            format!("systemctl --user start {ptyd}"),
            format!("systemctl --user start {engine}")
        ]
    );
    // The window's supervisor holds its lock: the unit is not started beside it.
    let lock = soyeht_platform::fs::lock_singleton(&layout.data, Component::Ptyd.lock()).unwrap();
    let held = idle_services(system);
    Manager::new(&layout, &held)
        .start(&[Component::Engine])
        .unwrap();
    assert_eq!(starts(&held), [format!("systemctl --user start {engine}")]);
    drop(lock);
}

#[test]
fn an_engine_only_start_on_linux_asks_for_no_supervisor_that_is_not_registered() {
    let scratch = Scratch::new("linux-no-supervisor");
    let layout = test_layout(System::Linux, &scratch.0);
    // Only the engine is registered: the supervisor has no unit file.
    let idle = idle_services(System::Linux);
    Manager::new(&layout, &idle)
        .install(&programs(), &[Component::Engine])
        .unwrap();
    let recorder = idle_services(System::Linux);
    Manager::new(&layout, &recorder)
        .start(&[Component::Engine])
        .unwrap();
    let ptyd = layout.profile.ptyd_systemd_unit;
    assert!(
        !mentions(&recorder, &format!("start {ptyd}")),
        "{:?}",
        recorder.lines()
    );
    assert!(
        mentions(&recorder, " start "),
        "the engine itself is started"
    );
}
