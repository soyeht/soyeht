use super::*;
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, PartialEq)]
enum Lock {
    Free,
    /// Another window holds it.
    Held,
    Broken,
}

/// A host that records what is started and lets the test say what answers.
struct Fake {
    engine_up: Cell<bool>,
    /// An uninstall ended the programs while the window was open.
    uninstalled: bool,
    supervisor_up: Cell<bool>,
    missing: Vec<&'static str>,
    lock: Lock,
    spawn_fails: bool,
    /// The folder for the logs cannot be made.
    logs_fail: bool,
    /// The started program has already ended.
    exits: bool,
    /// The engine answers after this many pauses (with the lock held, from the start).
    engine_after: Option<u32>,
    /// The engine answers once the lock has been tried, as when another window won the race.
    engine_after_lock: bool,
    stop_after: Option<u32>,
    pauses: Cell<u32>,
    locked: Cell<bool>,
    started: RefCell<Vec<String>>,
    /// The supervisor each started program was told to restart.
    restart: RefCell<Vec<Option<PathBuf>>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            engine_up: Cell::new(false),
            uninstalled: false,
            supervisor_up: Cell::new(false),
            missing: Vec::new(),
            lock: Lock::Free,
            spawn_fails: false,
            logs_fail: false,
            exits: false,
            engine_after: None,
            engine_after_lock: false,
            stop_after: None,
            pauses: Cell::new(0),
            locked: Cell::new(false),
            started: RefCell::new(Vec::new()),
            restart: RefCell::new(Vec::new()),
        }
    }

    fn started(&self) -> Vec<String> {
        self.started.borrow().clone()
    }
}

struct Child(bool);

impl Started for Child {
    fn exited(&mut self) -> bool {
        self.0
    }
}

impl Host for Fake {
    fn uninstalled(&self) -> bool {
        self.uninstalled
    }

    fn engine_reachable(&self) -> bool {
        self.engine_up.get()
            || (self.engine_after_lock && self.locked.get())
            || self
                .engine_after
                .is_some_and(|after| self.pauses.get() >= after)
    }

    fn supervisor_reachable(&self) -> bool {
        self.supervisor_up.get()
            || (!self.exits && self.started().iter().any(|name| name == "soyeht-ptyd"))
    }

    fn program_exists(&self, path: &Path) -> bool {
        !self.missing.iter().any(|name| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(name)
        })
    }

    fn lock(&self) -> io::Result<Option<Box<dyn Any>>> {
        self.locked.set(true);
        match self.lock {
            Lock::Free => Ok(Some(Box::new(()))),
            Lock::Held => Ok(None),
            Lock::Broken => Err(io::ErrorKind::PermissionDenied.into()),
        }
    }

    fn prepare_logs(&self) -> io::Result<()> {
        if self.logs_fail {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }

    fn start(
        &self,
        program: &Path,
        _log_name: &str,
        restart_supervisor: Option<&Path>,
    ) -> io::Result<Box<dyn Started>> {
        if self.spawn_fails {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let name = program.file_name().unwrap().to_string_lossy();
        self.started
            .borrow_mut()
            .push(name.trim_end_matches(EXE_SUFFIX).to_owned());
        self.restart
            .borrow_mut()
            .push(restart_supervisor.map(Path::to_owned));
        Ok(Box::new(Child(self.exits)))
    }

    fn pause(&self) -> bool {
        self.pauses.set(self.pauses.get() + 1);
        self.stop_after
            .is_none_or(|after| self.pauses.get() < after)
    }
}

/// The start succeeds and no program was started.
fn ensured_without_starting(host: &Fake) {
    assert_eq!(ensure(&plan(), host), Ok(()));
    assert!(host.started().is_empty());
}

fn plan() -> Plan {
    Plan {
        directory: PathBuf::from("/data"),
        programs: Some(PathBuf::from("/app")),
    }
}

#[test]
fn an_engine_that_answers_is_left_alone() {
    let host = Fake {
        engine_up: Cell::new(true),
        missing: vec!["soyeht-engine", "soyeht-ptyd"],
        ..Fake::new()
    };
    ensured_without_starting(&host);
    assert!(!host.locked.get());
}

#[test]
fn the_supervisor_then_the_engine_are_started_and_waited_for() {
    let host = Fake {
        engine_after: Some(3),
        ..Fake::new()
    };
    assert_eq!(ensure(&plan(), &host), Ok(()));
    assert_eq!(host.started(), ["soyeht-ptyd", "soyeht-engine"]);
}

#[test]
fn nothing_is_started_after_an_uninstall_ended_the_programs() {
    let host = Fake {
        uninstalled: true,
        ..Fake::new()
    };
    assert_eq!(ensure(&plan(), &host), Err(StartError::Uninstalled));
    assert!(host.started().is_empty());
    assert!(!host.locked.get());
}

#[test]
fn an_engine_that_answers_is_left_alone_even_after_an_uninstall_was_seen() {
    let host = Fake {
        uninstalled: true,
        engine_up: Cell::new(true),
        ..Fake::new()
    };
    ensured_without_starting(&host);
}

#[test]
fn an_uninstall_is_seen_by_the_record_or_by_the_data_going() {
    let directory = std::env::temp_dir().join(format!("next-autostart-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(fs::ENGINE_LOCK), "1").unwrap();
    let began = SystemTime::now() - Duration::from_secs(5);
    // Nothing happened, whether or not an engine ever answered.
    assert!(!uninstall_seen(&directory, began, false));
    assert!(!uninstall_seen(&directory, began, true));
    // The record, left after the window began.
    fs::mark_uninstalled(&directory).unwrap();
    assert!(uninstall_seen(&directory, began, false));
    // A window that began after it is a new start.
    assert!(!uninstall_seen(
        &directory,
        SystemTime::now() + Duration::from_secs(5),
        false
    ));
    // The data deleted takes the engine's lock file with it, and the folder can stay. That counts
    // only for a window that had an engine there: a first run has no lock file yet.
    std::fs::remove_file(directory.join(fs::UNINSTALL_MARKER)).unwrap();
    assert!(!uninstall_seen(&directory, began, true));
    std::fs::remove_file(directory.join(fs::ENGINE_LOCK)).unwrap();
    assert!(uninstall_seen(&directory, began, true));
    assert!(!uninstall_seen(&directory, began, false));
    std::fs::remove_dir_all(&directory).unwrap();
}

/// What each program started was told to restart, when the engine starts after `host` is set up.
fn restart_told(supervisor_up: bool, missing: Vec<&'static str>) -> Vec<Option<PathBuf>> {
    let host = Fake {
        supervisor_up: Cell::new(supervisor_up),
        missing,
        engine_after: Some(3),
        ..Fake::new()
    };
    assert_eq!(ensure(&plan(), &host), Ok(()));
    host.restart.borrow().clone()
}

#[test]
fn the_engine_is_told_how_to_start_the_supervisor_again_and_the_supervisor_is_not() {
    let supervisor = Some(PathBuf::from("/app").join(format!("soyeht-ptyd{EXE_SUFFIX}")));
    assert_eq!(restart_told(false, vec![]), [None, supervisor.clone()]);
    // A supervisor that answers is still the one the engine may start again later.
    assert_eq!(restart_told(true, vec![]), [supervisor]);
    // Without the program next to the window there is nothing to tell the engine.
    assert_eq!(restart_told(true, vec!["soyeht-ptyd"]), [None]);
}

#[test]
fn a_running_supervisor_is_not_started_again_and_needs_no_program_of_its_own() {
    for missing in [vec![], vec!["soyeht-ptyd"]] {
        let host = Fake {
            supervisor_up: Cell::new(true),
            missing,
            engine_after: Some(1),
            ..Fake::new()
        };
        assert_eq!(ensure(&plan(), &host), Ok(()));
        assert_eq!(host.started(), ["soyeht-engine"]);
    }
}

#[test]
fn a_missing_program_is_named_and_nothing_is_started() {
    for missing in ["soyeht-engine", "soyeht-ptyd"] {
        let host = Fake {
            missing: vec![missing],
            ..Fake::new()
        };
        assert_eq!(
            ensure(&plan(), &host),
            Err(StartError::MissingProgram(missing))
        );
        assert!(host.started().is_empty());
    }
}

#[test]
fn a_logs_folder_that_cannot_be_made_is_a_directory_error_and_nothing_is_started() {
    let host = Fake {
        logs_fail: true,
        ..Fake::new()
    };
    assert_eq!(
        ensure(&plan(), &host),
        Err(StartError::Directory(io::ErrorKind::PermissionDenied))
    );
    assert!(host.started().is_empty());
}

#[test]
fn an_unknown_program_folder_is_a_missing_program() {
    let plan = Plan {
        programs: None,
        ..plan()
    };
    assert_eq!(
        ensure(&plan, &Fake::new()),
        Err(StartError::MissingProgram("soyeht-engine"))
    );
}

#[test]
fn a_window_that_loses_the_race_waits_for_the_other_and_starts_nothing() {
    let host = Fake {
        lock: Lock::Held,
        engine_after: Some(5),
        ..Fake::new()
    };
    ensured_without_starting(&host);
    assert_eq!(host.pauses.get(), 5);
}

#[test]
fn an_engine_that_came_up_while_the_lock_was_taken_is_not_started_again() {
    let host = Fake {
        engine_after_lock: true,
        ..Fake::new()
    };
    ensured_without_starting(&host);
}

#[test]
fn a_start_that_cannot_be_made_is_an_error_of_its_kind() {
    let spawn = Fake {
        spawn_fails: true,
        ..Fake::new()
    };
    assert_eq!(
        ensure(&plan(), &spawn),
        Err(StartError::Spawn(
            "soyeht-ptyd",
            io::ErrorKind::PermissionDenied
        ))
    );
    let lock = Fake {
        lock: Lock::Broken,
        ..Fake::new()
    };
    assert_eq!(
        ensure(&plan(), &lock),
        Err(StartError::Lock(io::ErrorKind::PermissionDenied))
    );
}

#[test]
fn a_program_that_ends_at_once_is_told_by_name() {
    let supervisor = Fake {
        exits: true,
        ..Fake::new()
    };
    assert_eq!(
        ensure(&plan(), &supervisor),
        Err(StartError::Exited("soyeht-ptyd"))
    );
    let engine = Fake {
        exits: true,
        supervisor_up: Cell::new(true),
        ..Fake::new()
    };
    assert_eq!(
        ensure(&plan(), &engine),
        Err(StartError::Exited("soyeht-engine"))
    );
}

#[test]
fn an_engine_that_never_answers_times_out_and_a_stopped_application_ends_the_wait() {
    let silent = Fake {
        supervisor_up: Cell::new(true),
        ..Fake::new()
    };
    assert_eq!(ensure(&plan(), &silent), Err(StartError::TimedOut));
    assert_eq!(silent.pauses.get(), POLLS);
    let stopped = Fake {
        supervisor_up: Cell::new(true),
        stop_after: Some(2),
        ..Fake::new()
    };
    assert_eq!(ensure(&plan(), &stopped), Err(StartError::Stopped));
}

/// The data directory of the Dev profile below a fixed base, as the profile module finds it.
fn data() -> crate::Result<PathBuf> {
    soyeht_protocol::data_directory::data_directory(
        &soyeht_protocol::profile::DEV,
        Some("/data".into()),
    )
    .map_err(crate::Error::from)
}

#[test]
fn an_environment_socket_turns_the_start_off() {
    let exe = || Ok(PathBuf::from("/app/soyeht-desktop"));
    assert_eq!(
        Autostart::from_parts(Some("/run/x.sock".into()), data(), exe()),
        Autostart::Off
    );
    // An empty value is the same as none, as for the socket itself.
    let on = Autostart::from_parts(Some(OsString::new()), data(), exe());
    assert!(matches!(on, Autostart::On(_)));
    assert_eq!(on, Autostart::from_parts(None, data(), exe()));
}

#[test]
fn the_plan_uses_the_profile_directory_and_the_folder_of_the_executable() {
    let on = Autostart::from_parts(None, data(), Ok(PathBuf::from("/app/soyeht-desktop")));
    let Autostart::On(plan) = &on else {
        panic!("expected a start");
    };
    assert_eq!(plan.programs, Some(PathBuf::from("/app")));
    assert_eq!(plan.directory, data().unwrap());
    let unknown = Autostart::from_parts(None, data(), Err(io::ErrorKind::NotFound.into()));
    assert!(matches!(
        unknown,
        Autostart::On(Plan { programs: None, .. })
    ));
    let no_base =
        soyeht_protocol::data_directory::data_directory(&soyeht_protocol::profile::DEV, None)
            .map_err(crate::Error::from);
    assert_eq!(
        Autostart::from_parts(None, no_base, Ok("/app/x".into())),
        Autostart::Off
    );
}

#[test]
fn the_log_keeps_the_kind_and_the_message_names_no_path() {
    use log::Failure as _;
    let error = StartError::Spawn("soyeht-engine", io::ErrorKind::PermissionDenied);
    assert_eq!(error.failure_kind(), "spawn.PermissionDenied");
    assert_eq!(StartError::TimedOut.failure_kind(), "timed_out");
    // A window that only waited for another window's start must not claim to have started one.
    assert!(!StartError::TimedOut.to_string().contains("started"));
    for error in [
        StartError::MissingProgram("soyeht-engine"),
        StartError::Exited("soyeht-ptyd"),
        StartError::Directory(io::ErrorKind::NotFound),
        error,
    ] {
        assert!(!error.to_string().contains('/'), "{error}");
    }
}
