use super::*;
use crate::{
    commands::{
        CommandOutput,
        fake::{Recorder, no, ok},
    },
    layout::test_layout,
};
use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

/// A folder of its own for one test; its name carries "next" like every place of the app.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("next-service-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A folder with the programs of every system, made once for the whole run.
fn programs() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder =
                std::env::temp_dir().join(format!("next-service-programs-{}", std::process::id()));
            fs::create_dir_all(&folder).unwrap();
            for name in [
                "soyeht-ptyd",
                "soyeht-engine",
                "soyeht-ptyd.exe",
                "soyeht-engine.exe",
            ] {
                fs::write(folder.join(name), b"program").unwrap();
            }
            folder
        })
        .clone()
}

/// A scratch folder and the layout in it, with `components` installed through `fake`.
fn installed(
    system: System,
    label: &str,
    fake: &Recorder,
    components: &[Component],
) -> (Scratch, Layout) {
    let scratch = Scratch::new(label);
    let layout = test_layout(system, &scratch.0);
    Manager::new(&layout, fake)
        .install(&programs(), components)
        .unwrap();
    (scratch, layout)
}

const BOTH: [Component; 2] = [Component::Engine, Component::Ptyd];

/// A launchd that has a graphical domain or not, and loads a job when it is bootstrapped and
/// forgets it when it is booted out. `loaded` says whether every job is loaded before anything
/// runs.
fn launchd(graphical: bool, loaded: bool) -> Recorder {
    let changed: Rc<RefCell<HashMap<String, bool>>> = Rc::default();
    let label = |text: &str| {
        text.rsplit('/')
            .next()
            .unwrap_or(text)
            .trim_end_matches(".plist")
            .to_owned()
    };
    Recorder::answering(move |call| match call {
        ["launchctl", "bootstrap", _, file] => {
            changed.borrow_mut().insert(label(file), true);
            ok("")
        }
        ["launchctl", "bootout", target] => {
            changed.borrow_mut().insert(label(target), false);
            ok("")
        }
        // The domain has two parts (`gui/501`), a job three.
        ["launchctl", "print", target] if target.matches('/').count() == 1 => {
            if graphical && target.starts_with("gui/") {
                ok("")
            } else {
                no()
            }
        }
        ["launchctl", "print", target] => {
            if changed
                .borrow()
                .get(&label(target))
                .copied()
                .unwrap_or(loaded)
            {
                ok("state = not running\n")
            } else {
                no()
            }
        }
        _ => ok(""),
    })
}

fn mentions(recorder: &Recorder, text: &str) -> bool {
    recorder.lines().iter().any(|line| line.contains(text))
}

#[test]
fn macos_install_writes_the_agents_and_loads_each_in_order() {
    let scratch = Scratch::new("mac-install");
    let layout = test_layout(System::Macos, &scratch.0);
    let recorder = launchd(true, false);
    Manager::new(&layout, &recorder)
        .install(&programs(), &BOTH)
        .unwrap();
    let lines = recorder.lines();
    let bootstraps: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("bootstrap"))
        .collect();
    assert_eq!(bootstraps.len(), 2);
    assert!(
        bootstraps[0].contains(layout.profile.ptyd_launchd_label),
        "the supervisor first"
    );
    assert!(bootstraps[1].contains(layout.profile.engine_launchd_label));
    assert!(bootstraps[0].starts_with("launchctl bootstrap gui/501 "));
    for component in BOTH {
        let written = fs::read_to_string(layout.definition_file(component)).unwrap();
        let definition = layout.definition(component, &programs()).unwrap();
        assert_eq!(written, render(System::Macos, &definition).unwrap());
        assert!(layout.log(component).is_file());
    }
}

#[cfg(unix)]
#[test]
fn what_install_writes_is_for_the_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_scratch, layout) = installed(
        System::Macos,
        "owner-only",
        &launchd(true, false),
        &[Component::Ptyd],
    );
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&layout.data), 0o700);
    assert_eq!(mode(&layout.data.join("logs")), 0o700);
    assert_eq!(mode(&layout.log(Component::Ptyd)), 0o600);
    assert_eq!(mode(&layout.definition_file(Component::Ptyd)), 0o600);
}

#[test]
fn an_agent_that_is_loaded_is_rewritten_and_not_loaded_again() {
    let scratch = Scratch::new("mac-loaded");
    let layout = test_layout(System::Macos, &scratch.0);
    let recorder = Recorder::answering(|_| ok("state = running"));
    Manager::new(&layout, &recorder)
        .install(&programs(), &[Component::Engine])
        .unwrap();
    assert!(!mentions(&recorder, "bootstrap") && !mentions(&recorder, "bootout"));
    assert!(layout.definition_file(Component::Engine).is_file());
}

#[test]
fn an_agent_that_is_loaded_and_idle_is_loaded_again_with_the_new_text() {
    let scratch = Scratch::new("mac-idle");
    let layout = test_layout(System::Macos, &scratch.0);
    let recorder = launchd(true, true);
    Manager::new(&layout, &recorder)
        .install(&programs(), &[Component::Engine])
        .unwrap();
    let lines = recorder.lines();
    let out = lines
        .iter()
        .position(|line| line.contains("bootout"))
        .unwrap();
    let load = lines
        .iter()
        .position(|line| line.contains("bootstrap"))
        .unwrap();
    assert!(out < load, "{lines:?}");
}

#[test]
fn a_missing_program_is_refused_before_anything_is_written() {
    for system in [System::Macos, System::Linux, System::Windows] {
        let scratch = Scratch::new(&format!("missing-{}", system.name()));
        let layout = test_layout(system, &scratch.0);
        let recorder = Recorder::default();
        let error = Manager::new(&layout, &recorder)
            .install(&scratch.0.join("no-such-folder"), &BOTH)
            .unwrap_err();
        assert!(
            matches!(error, Error::ProgramMissing(_)),
            "{system:?}: {error}"
        );
        assert!(recorder.lines().is_empty());
        assert!(!layout.data.exists());
    }
}

#[test]
fn macos_uses_the_background_domain_when_there_is_no_graphical_one() {
    let scratch = Scratch::new("mac-background");
    let layout = test_layout(System::Macos, &scratch.0);
    let recorder = launchd(false, false);
    Manager::new(&layout, &recorder)
        .install(&programs(), &[Component::Ptyd])
        .unwrap();
    assert!(mentions(&recorder, "launchctl bootstrap user/501 "));
}

#[test]
fn macos_uninstall_unloads_the_engine_first_and_removes_the_files() {
    let (_scratch, layout) =
        installed(System::Macos, "mac-uninstall", &launchd(true, false), &BOTH);
    let recorder = launchd(true, true);
    Manager::new(&layout, &recorder).uninstall(&BOTH).unwrap();
    let boots: Vec<String> = recorder
        .lines()
        .into_iter()
        .filter(|line| line.contains("bootout"))
        .collect();
    assert_eq!(boots.len(), 2);
    assert!(boots[0].ends_with(layout.profile.engine_launchd_label));
    assert!(boots[1].ends_with(layout.profile.ptyd_launchd_label));
    for component in BOTH {
        assert!(!layout.definition_file(component).exists());
    }
    // The data directory is not the service's to remove.
    assert!(layout.data.is_dir());
    // Removing again finds nothing and is not an error.
    Manager::new(&layout, &launchd(true, false))
        .uninstall(&BOTH)
        .unwrap();
}

#[test]
fn macos_status_reads_the_state_of_the_loaded_job() {
    let (_scratch, layout) = installed(
        System::Macos,
        "mac-status",
        &launchd(true, false),
        &[Component::Engine],
    );
    let state = |text: &'static str| {
        let recorder = Recorder::answering(move |_| ok(text));
        Manager::new(&layout, &recorder)
            .status(Component::Engine)
            .unwrap()
    };
    assert!(state("state = running\n").running);
    assert!(!state("state = not running\n").running);
    let status = state("state = running\n");
    assert!(status.registered);
    assert_eq!(status.name, layout.profile.engine_launchd_label);
    let nothing = Recorder::answering(|_| no());
    let status = Manager::new(&layout, &nothing)
        .status(Component::Ptyd)
        .unwrap();
    assert!(!status.registered && !status.running);
}

#[test]
fn start_without_a_registration_runs_nothing_and_says_so() {
    for system in [System::Macos, System::Linux, System::Windows] {
        let scratch = Scratch::new(&format!("unregistered-{}", system.name()));
        let layout = test_layout(system, &scratch.0);
        let recorder = Recorder::answering(|_| no());
        let error = Manager::new(&layout, &recorder).start(&BOTH).unwrap_err();
        assert!(matches!(error, Error::NotRegistered), "{system:?}: {error}");
        for started in ["kickstart", "bootstrap", " start ", "/Run"] {
            assert!(
                !mentions(&recorder, started),
                "{system:?}: {:?}",
                recorder.lines()
            );
        }
    }
}

#[test]
fn a_registered_agent_that_is_not_loaded_is_started_by_loading_it() {
    let (_scratch, layout) = installed(
        System::Macos,
        "mac-start",
        &launchd(true, false),
        &[Component::Ptyd],
    );
    let recorder = launchd(true, false);
    Manager::new(&layout, &recorder)
        .start(&[Component::Ptyd])
        .unwrap();
    assert!(mentions(&recorder, "launchctl bootstrap gui/501 "));
    // A running agent is left alone.
    let running = Recorder::answering(|_| ok("state = running"));
    Manager::new(&layout, &running)
        .start(&[Component::Ptyd])
        .unwrap();
    assert!(!mentions(&running, "kickstart") && !mentions(&running, "bootstrap"));
    // A loaded agent that is not running is kicked.
    let idle = launchd(true, true);
    Manager::new(&layout, &idle)
        .start(&[Component::Ptyd])
        .unwrap();
    assert!(mentions(&idle, "launchctl kickstart gui/501/"));
}

#[test]
fn linux_install_writes_the_units_and_enables_each_now() {
    let scratch = Scratch::new("linux-install");
    let layout = test_layout(System::Linux, &scratch.0);
    let recorder = Recorder::default();
    Manager::new(&layout, &recorder)
        .install(&programs(), &BOTH)
        .unwrap();
    let ptyd = layout.profile.ptyd_systemd_unit;
    let engine = layout.profile.engine_systemd_unit;
    assert_eq!(
        recorder.lines(),
        [
            "systemctl --user daemon-reload".to_owned(),
            format!("systemctl --user enable --now {ptyd}"),
            "systemctl --user daemon-reload".to_owned(),
            format!("systemctl --user enable --now {engine}"),
        ]
    );
    let unit = fs::read_to_string(layout.definition_file(Component::Engine)).unwrap();
    assert!(unit.contains(&format!("After={ptyd}\n")));
    assert!(unit.contains("\"--profile\" \"prod\""));
}

#[test]
fn linux_uninstall_disables_each_unit_and_reloads() {
    let (_scratch, layout) = installed(
        System::Linux,
        "linux-uninstall",
        &Recorder::default(),
        &BOTH,
    );
    let recorder = Recorder::default();
    Manager::new(&layout, &recorder).uninstall(&BOTH).unwrap();
    let engine = layout.profile.engine_systemd_unit;
    let ptyd = layout.profile.ptyd_systemd_unit;
    assert_eq!(
        recorder.lines(),
        [
            format!("systemctl --user disable --now {engine}"),
            "systemctl --user daemon-reload".to_owned(),
            format!("systemctl --user disable --now {ptyd}"),
            "systemctl --user daemon-reload".to_owned(),
        ]
    );
    assert!(!layout.definition_file(Component::Engine).exists());
}

#[test]
fn linux_status_needs_the_file_and_asks_systemd() {
    let scratch = Scratch::new("linux-status");
    let layout = test_layout(System::Linux, &scratch.0);
    let silent = Recorder::answering(|_| ok(""));
    let before = Manager::new(&layout, &silent)
        .status(Component::Ptyd)
        .unwrap();
    assert!(!before.registered && !before.running);
    assert!(silent.lines().is_empty(), "no unit file, nothing to ask");
    Manager::new(&layout, &Recorder::default())
        .install(&programs(), &[Component::Ptyd])
        .unwrap();
    let enabled_only = Recorder::answering(|call| {
        if call.contains(&"is-enabled") {
            ok("enabled")
        } else {
            no()
        }
    });
    let status = Manager::new(&layout, &enabled_only)
        .status(Component::Ptyd)
        .unwrap();
    assert!(status.registered && !status.running);
}

#[test]
fn linux_stop_goes_engine_first_and_start_leaves_an_active_unit_alone() {
    let (_scratch, layout) = installed(System::Linux, "linux-stop", &Recorder::default(), &BOTH);
    let recorder = Recorder::default();
    Manager::new(&layout, &recorder).stop(&BOTH).unwrap();
    let lines = recorder.lines();
    assert!(lines[0].ends_with(layout.profile.engine_systemd_unit));
    assert!(lines[1].ends_with(layout.profile.ptyd_systemd_unit));
    let running = Recorder::answering(|_| ok("active"));
    Manager::new(&layout, &running).start(&BOTH).unwrap();
    assert!(!mentions(&running, " start "));
}

/// PowerShell answers the question about the task's state; every other command succeeds.
fn task_scheduler(state: &'static str) -> Recorder {
    Recorder::answering(move |call| {
        if call[0] == "powershell.exe" {
            ok(state)
        } else {
            ok("")
        }
    })
}

#[test]
fn windows_install_registers_each_task_from_a_utf16_copy_and_runs_it() {
    let scratch = Scratch::new("windows-install");
    let layout = test_layout(System::Windows, &scratch.0);
    let recorder = task_scheduler("stopped\r\n");
    Manager::new(&layout, &recorder)
        .install(&programs(), &BOTH)
        .unwrap();
    let lines = recorder.lines();
    let ptyd_file = layout.definition_file(Component::Ptyd);
    assert_eq!(
        lines[0],
        format!(
            "schtasks /Create /TN {} /XML {} /F",
            layout.profile.ptyd_windows_task,
            ptyd_file.display()
        )
    );
    assert!(lines.contains(&format!(
        "schtasks /Run /TN {}",
        layout.profile.engine_windows_task
    )));
    let bytes = fs::read(&ptyd_file).unwrap();
    assert_eq!(&bytes[..2], [0xFF, 0xFE]);
    let units: Vec<u16> = bytes[2..]
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let xml = String::from_utf16(&units).unwrap();
    assert!(xml.contains("<LogonTrigger>"));
    assert!(xml.contains("S-1-5-21-1-2-3-1001"));
    // A task has no log file to prepare.
    assert!(!layout.data.join("logs").exists());
}

#[test]
fn windows_leaves_a_running_task_running_and_removes_a_registered_one() {
    let scratch = Scratch::new("windows-running");
    let layout = test_layout(System::Windows, &scratch.0);
    let running = task_scheduler("running\r\n");
    Manager::new(&layout, &running)
        .install(&programs(), &[Component::Engine])
        .unwrap();
    assert!(!mentions(&running, "/Run"));
    let recorder = task_scheduler("running");
    Manager::new(&layout, &recorder)
        .uninstall(&[Component::Engine])
        .unwrap();
    let lines = recorder.lines();
    let name = layout.profile.engine_windows_task;
    assert!(lines.contains(&format!("schtasks /End /TN {name}")));
    assert!(lines.contains(&format!("schtasks /Delete /TN {name} /F")));
    assert!(!layout.definition_file(Component::Engine).exists());
    // A task the scheduler does not have is not an error to remove.
    let none = Recorder::answering(|_| no());
    Manager::new(&layout, &none)
        .uninstall(&[Component::Engine])
        .unwrap();
    assert_eq!(none.lines().len(), 1);
}

#[test]
fn windows_status_does_not_depend_on_the_language_of_the_system() {
    let scratch = Scratch::new("windows-status");
    let layout = test_layout(System::Windows, &scratch.0);
    let recorder = Recorder::answering(|call| {
        if call[0] == "powershell.exe" {
            ok("running\n")
        } else {
            // The scheduler's own text is in the system's language; only its exit code is read.
            ok("Pronto")
        }
    });
    let status = Manager::new(&layout, &recorder)
        .status(Component::Ptyd)
        .unwrap();
    assert!(status.registered && status.running);
    let script = recorder.calls.borrow()[1][4].clone();
    assert!(script.contains(layout.profile.ptyd_windows_task));
}

#[test]
fn a_command_that_fails_stops_the_install_and_says_which_step() {
    let scratch = Scratch::new("failure");
    let layout = test_layout(System::Linux, &scratch.0);
    let recorder = Recorder::answering(|_| CommandOutput {
        success: false,
        stdout: String::new(),
        stderr: "Failed to connect to bus".into(),
    });
    let error = Manager::new(&layout, &recorder)
        .install(&programs(), &BOTH)
        .unwrap_err();
    assert!(matches!(error, Error::Failed { .. }));
    assert_eq!(
        error.to_string(),
        "systemctl daemon-reload failed: Failed to connect to bus"
    );
    assert_eq!(recorder.lines().len(), 1, "nothing else ran");
}

#[test]
fn nothing_is_written_when_a_definition_is_refused() {
    let scratch = Scratch::new("legacy");
    let mut layout = test_layout(System::Macos, &scratch.0);
    layout.data = crate::guard::legacy_values::data(&scratch.0);
    let recorder = Recorder::default();
    let error = Manager::new(&layout, &recorder)
        .install(&programs(), &BOTH)
        .unwrap_err();
    assert!(matches!(error, Error::Legacy(_)));
    assert!(recorder.lines().is_empty());
    assert!(!layout.data.exists());
    assert!(!layout.definition_file(Component::Ptyd).exists());
}

#[cfg(unix)]
#[test]
fn a_link_where_the_data_folder_belongs_is_refused() {
    let scratch = Scratch::new("link");
    let layout = test_layout(System::Macos, &scratch.0);
    fs::create_dir_all(scratch.0.join("elsewhere")).unwrap();
    fs::create_dir_all(layout.data.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(scratch.0.join("elsewhere"), &layout.data).unwrap();
    let recorder = Recorder::default();
    let error = Manager::new(&layout, &recorder)
        .install(&programs(), &[Component::Ptyd])
        .unwrap_err();
    assert!(matches!(error, Error::Unsafe(_)));
    assert!(recorder.lines().is_empty());
}

#[test]
fn components_are_taken_once_in_start_order_whatever_the_caller_passes() {
    assert_eq!(
        ordered(&[Component::Engine, Component::Ptyd, Component::Engine]),
        [Component::Ptyd, Component::Engine]
    );
    assert_eq!(ordered(&[Component::Engine]), [Component::Engine]);
}
