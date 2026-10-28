#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The host starts a real program, appends its output to the log and exits with its code.

use std::{fs, path::PathBuf, process::Command};

fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("next-host-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

fn host() -> Command {
    Command::new(env!("CARGO_BIN_EXE_soyeht-service-host"))
}

#[cfg(unix)]
fn shell(script: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), script.into()]
}

#[cfg(windows)]
fn shell(script: &str) -> Vec<String> {
    vec!["cmd".into(), "/c".into(), script.into()]
}

#[test]
fn the_programs_output_goes_to_the_log_and_its_exit_code_is_the_hosts() {
    let folder = scratch("exit");
    let log = folder.join("logs/engine.log");
    // The log's folder does not exist yet: the host makes it.
    let status = host()
        .arg("--log")
        .arg(&log)
        .arg("--")
        .args(shell(
            "echo from-the-program && echo and-its-error 1>&2 && exit 7",
        ))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(7));
    let text = fs::read_to_string(&log).unwrap();
    assert!(
        text.contains("from-the-program") && text.contains("and-its-error"),
        "{text:?}"
    );
    // A second run appends.
    host()
        .arg("--log")
        .arg(&log)
        .arg("--")
        .args(shell("echo again"))
        .status()
        .unwrap();
    assert!(fs::read_to_string(&log).unwrap().contains("again"));
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_program_that_cannot_start_fails_the_host_and_says_so_in_the_log() {
    let folder = scratch("missing");
    let log = folder.join("engine.log");
    let status = host()
        .arg("--log")
        .arg(&log)
        .arg("--")
        .arg(folder.join("no-such-program"))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    // The log file is made before the program starts, so the failure line can be written into it.
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.contains("service_host.start_failed"), "{text:?}");
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_misused_host_fails_without_starting_anything() {
    assert_eq!(host().arg("--nonsense").status().unwrap().code(), Some(2));
    assert_eq!(host().status().unwrap().code(), Some(2));
}

#[cfg(unix)]
#[test]
fn a_log_that_cannot_be_written_has_its_own_exit_code() {
    let folder = scratch("unwritable");
    // The log's name is a folder, so it cannot be opened for appending.
    let log = folder.join("engine.log");
    fs::create_dir(&log).unwrap();
    let status = host()
        .arg("--log")
        .arg(&log)
        .arg("--")
        .args(shell("exit 0"))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(3));
    fs::remove_dir_all(&folder).unwrap();
}
