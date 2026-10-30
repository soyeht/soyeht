//! A registered agent whose program is gone (contract 13, section 5).

use super::{tests::BOTH, *};
use crate::commands::fake::{Recorder, no, ok};

#[test]
fn macos_start_refuses_an_agent_whose_program_is_gone_instead_of_reporting_success() {
    let folder = std::env::temp_dir().join(format!("next-service-mac-gone-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    for name in ["soyeht-ptyd", "soyeht-engine"] {
        fs::write(folder.join(name), b"program").unwrap();
    }
    let scratch = crate::testing::Scratch::new("mac-start-gone");
    let layout = crate::layout::test_layout(System::Macos, &scratch.0);
    let fake = Recorder::answering(|_| ok("state = running"));
    Manager::new(&layout, &fake)
        .install(&folder, &BOTH)
        .unwrap();
    fs::remove_file(folder.join("soyeht-engine")).unwrap();
    let idle = Recorder::answering(|_| no());
    let result = Manager::new(&layout, &idle).start(&[Component::Engine]);
    assert!(
        matches!(&result, Err(Error::ProgramMissing(program)) if program.ends_with("soyeht-engine")),
        "{result:?}"
    );
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
