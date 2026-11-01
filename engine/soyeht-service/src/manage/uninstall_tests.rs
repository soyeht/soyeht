//! What an uninstall does to a program that still runs after its registration is gone
//! (contract 13, section 7, ADR 0131).

use super::{
    tests::{BOTH, installed, programs},
    *,
};
use crate::commands::fake::{Recorder, ok};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// The locks the programs of both components hold while they run, as they hold them.
type Held = Rc<RefCell<Vec<(Component, std::fs::File)>>>;

fn running_programs(layout: &Layout) -> Held {
    let held = Held::default();
    for component in [Component::Ptyd, Component::Engine] {
        let lock = soyeht_platform::fs::lock_singleton(&layout.data, component.lock()).unwrap();
        held.borrow_mut().push((component, lock));
    }
    held
}

#[test]
fn uninstall_ends_the_programs_the_window_started_engine_first() {
    let (_scratch, layout) = installed(
        System::Linux,
        "uninstall-running",
        &Recorder::default(),
        &BOTH,
    );
    let held = running_programs(&layout);
    // A program that is asked to end lets go of its lock; the engine is asked first, and is
    // gone before the supervisor is asked.
    let (asked, data) = (Rc::new(Cell::new(0)), layout.data.clone());
    let recorder = Recorder::default().ending_programs_with({
        let (held, asked) = (held.clone(), asked.clone());
        move |_| {
            asked.set(asked.get() + 1);
            let ended = if asked.get() == 1 {
                Component::Engine
            } else {
                assert!(
                    !soyeht_platform::fs::singleton_held(&data, Component::Engine.lock()).unwrap()
                );
                Component::Ptyd
            };
            held.borrow_mut()
                .retain(|(component, _)| *component != ended);
        }
    });
    Manager::new(&layout, &recorder).uninstall(&BOTH).unwrap();
    assert_eq!(*recorder.terminated.borrow(), [std::process::id(); 2]);
    assert!(held.borrow().is_empty());
    assert!(!layout.definition_file(Component::Engine).exists());
}

#[test]
fn uninstall_asks_nobody_when_no_program_runs() {
    let (_scratch, layout) =
        installed(System::Linux, "uninstall-idle", &Recorder::default(), &BOTH);
    let recorder = Recorder::default();
    Manager::new(&layout, &recorder).uninstall(&BOTH).unwrap();
    assert!(recorder.terminated.borrow().is_empty());
}

#[test]
fn uninstall_says_so_when_a_program_does_not_end_and_still_asks_the_other() {
    let (_scratch, layout) = installed(
        System::Linux,
        "uninstall-stuck",
        &Recorder::default(),
        &BOTH,
    );
    let held = running_programs(&layout);
    // Nothing lets go of its lock.
    let recorder = Recorder::default().ending_programs_with(|_| {});
    let error = Manager::new(&layout, &recorder)
        .uninstall(&BOTH)
        .unwrap_err();
    assert!(matches!(error, Error::StillRunning), "{error}");
    assert_eq!(recorder.terminated.borrow().len(), 2);
    // The services are unregistered all the same.
    assert!(!layout.definition_file(Component::Ptyd).exists());
    // It waited for the bound, in short steps, for each program.
    assert_eq!(
        recorder.pauses.borrow().len(),
        2 * (END_WAIT.as_millis() / END_POLL.as_millis()) as usize
    );
    drop(held);
}

#[test]
fn uninstall_unregisters_the_other_service_when_one_program_cannot_be_asked() {
    let (_scratch, layout) = installed(
        System::Linux,
        "uninstall-refused",
        &Recorder::default(),
        &BOTH,
    );
    let held = running_programs(&layout);
    // The engine's process is another account's: the system refuses. The supervisor is asked
    // all the same and ends.
    let recorder = Recorder::default()
        .refusing_the_first_end(io::ErrorKind::PermissionDenied)
        .ending_programs_with({
            let held = held.clone();
            move |_| {
                held.borrow_mut()
                    .retain(|(component, _)| *component != Component::Ptyd)
            }
        });
    let error = Manager::new(&layout, &recorder)
        .uninstall(&BOTH)
        .unwrap_err();
    assert!(
        matches!(&error, Error::Io(inner) if inner.kind() == io::ErrorKind::PermissionDenied),
        "{error}"
    );
    assert_eq!(recorder.terminated.borrow().len(), 2);
    assert!(!layout.definition_file(Component::Engine).exists());
    assert!(!layout.definition_file(Component::Ptyd).exists());
    drop(held);
}

#[test]
fn uninstall_leaves_a_record_that_install_takes_away() {
    let (_scratch, layout) = installed(
        System::Linux,
        "uninstall-record",
        &Recorder::default(),
        &BOTH,
    );
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    assert!(!soyeht_platform::fs::uninstalled_since(
        &layout.data,
        long_ago
    ));
    Manager::new(&layout, &Recorder::default())
        .uninstall(&BOTH)
        .unwrap();
    assert!(soyeht_platform::fs::uninstalled_since(
        &layout.data,
        long_ago
    ));
    Manager::new(&layout, &Recorder::default())
        .install(&programs(), &BOTH)
        .unwrap();
    assert!(!soyeht_platform::fs::uninstalled_since(
        &layout.data,
        long_ago
    ));
}

#[test]
fn uninstall_does_not_ask_a_program_that_ended_between_the_lock_and_the_request() {
    let (_scratch, layout) =
        installed(System::Linux, "uninstall-gone", &Recorder::default(), &BOTH);
    let held = running_programs(&layout);
    struct Gone(Held);
    impl Commands for Gone {
        fn run(&self, _: &str, _: &[&str]) -> Result<CommandOutput> {
            Ok(ok(""))
        }
        fn terminate(&self, _: u32) -> io::Result<()> {
            self.0.borrow_mut().clear();
            Err(io::ErrorKind::NotFound.into())
        }
    }
    Manager::new(&layout, &Gone(held.clone()))
        .uninstall(&BOTH)
        .unwrap();
    assert!(held.borrow().is_empty());
}
