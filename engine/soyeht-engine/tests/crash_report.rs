//! An engine that panics on its main thread leaves a crash report in its data directory
//! (ADR 0127), and the report is what the clients' notice and `soyeht diagnostics` read.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::crash::{Program, store};
use std::{
    fs,
    process::{Command, Stdio},
};

#[test]
fn a_panic_on_the_main_thread_ends_the_engine_and_leaves_a_report() {
    let data = std::env::temp_dir().join(format!("soyeht-engine-crash-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data);
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("test-crash-at-start"), "").unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_soyeht-engine-test"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success(), "the engine still ends when it panics");

    let pending = store::pending(&data).unwrap();
    assert_eq!(pending.len(), 1, "{:?}", store::list(&data));
    let report = &pending[0].report;
    assert_eq!(report.program, Program::Engine);
    assert!(report.fatal);
    assert_eq!(report.thread, "main");
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
    assert!(
        report.location.contains("program.rs"),
        "{}",
        report.location
    );
    assert!(!report.frames.is_empty());
    assert!(
        !fs::read_to_string(store::directory(&data).join(&pending[0].name))
            .unwrap()
            .contains(&data.display().to_string())
    );
    fs::remove_dir_all(data).unwrap();
}
