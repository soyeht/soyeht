//! A registered service whose program is gone (contract 13, section 5).

use super::{tests::BOTH, *};
use crate::{
    commands::fake::{Recorder, no, ok},
    layout::test_layout,
    testing::Scratch,
};

/// `system`'s services installed from a folder, then the engine's program removed from it.
fn engine_program_removed(
    system: System,
    label: &str,
    installer: &Recorder,
) -> (Scratch, Layout, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!(
        "next-service-programs-{label}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&folder).unwrap();
    for name in ["soyeht-ptyd", "soyeht-engine"] {
        fs::write(folder.join(name), b"program").unwrap();
    }
    let scratch = Scratch::new(label);
    let layout = test_layout(system, &scratch.0);
    Manager::new(&layout, installer)
        .install(&folder, &BOTH)
        .unwrap();
    fs::remove_file(folder.join("soyeht-engine")).unwrap();
    (scratch, layout, folder)
}

fn assert_missing(result: Result<()>) {
    assert!(
        matches!(&result, Err(Error::ProgramMissing(program)) if program.ends_with("soyeht-engine")),
        "{result:?}"
    );
}

#[test]
fn linux_start_refuses_a_unit_whose_program_is_gone_instead_of_reporting_success() {
    let (_scratch, layout, folder) =
        engine_program_removed(System::Linux, "linux-start-gone", &Recorder::default());
    let inactive = Recorder::answering(|_| no());
    assert_missing(Manager::new(&layout, &inactive).start(&[Component::Engine]));
    assert!(!inactive.lines().join("\n").contains(" start "));
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn macos_start_refuses_an_agent_whose_program_is_gone_instead_of_reporting_success() {
    let (_scratch, layout, folder) = engine_program_removed(
        System::Macos,
        "mac-start-gone",
        &Recorder::answering(|_| ok("state = running")),
    );
    let idle = Recorder::answering(|_| no());
    assert_missing(Manager::new(&layout, &idle).start(&[Component::Engine]));
    let lines = idle.lines().join("\n");
    assert!(
        !lines.contains("bootstrap") && !lines.contains("kickstart"),
        "{lines}"
    );
    // The supervisor's program is still there, so it starts as before.
    Manager::new(&layout, &Recorder::answering(|_| ok("state = running")))
        .start(&[Component::Ptyd])
        .unwrap();
    fs::remove_dir_all(&folder).unwrap();
}
