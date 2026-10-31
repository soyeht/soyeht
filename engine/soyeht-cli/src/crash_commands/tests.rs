use super::*;
use soyeht_platform::{crash::Program, testing::scratch_dir};

fn diagnostic() -> Value {
    json!({
        "version": "1.0.0",
        "diagnosticMetaData": {
            "appVersion": "0.1.0",
            "platformArchitecture": "arm64e",
            "signal": 11,
            "exceptionType": 1,
            "terminationReason": "Namespace SIGNAL, Code 0xb /Users/dana/Projects/secret",
            "bundleIdentifier": "com.example.private"
        },
        "callStackTree": {
            "callStackPerThread": true,
            "callStacks": [
                {"threadAttributed": false, "callStackRootFrames": [
                    {"binaryName": "other", "offsetIntoBinaryTextSegment": 1}]},
                {"threadAttributed": true, "callStackRootFrames": [
                    {"binaryName": "dyld", "offsetIntoBinaryTextSegment": 16, "subFrames": [
                        {"binaryName": "/Users/dana/Soyeht.app/Contents/MacOS/Soyeht",
                         "offsetIntoBinaryTextSegment": 4660, "subFrames": [
                            {"binaryName": "libswiftCore.dylib", "offsetIntoBinaryTextSegment": 255}
                        ]}]}]}
            ]
        }
    })
}

#[test]
fn a_system_diagnostic_becomes_a_report_with_facts_and_no_free_text() {
    let report = metrickit::report(&diagnostic(), 1_761_907_872);
    assert_eq!(report.program, Program::Mac);
    assert!(report.fatal);
    assert_eq!(report.kind, "signal_SIGSEGV");
    assert_eq!(report.version, "0.1.0");
    assert_eq!(report.system, "macos arm64e");
    assert_eq!(
        report.frames,
        vec!["libswiftCore.dylib+0xff", "Soyeht+0x1234", "dyld+0x10"]
    );
    let text = report.to_text();
    assert!(!text.contains("dana") && !text.contains("secret") && !text.contains("example"));
}

#[test]
fn a_diagnostic_with_nothing_in_it_is_still_a_report() {
    let report = metrickit::report(&json!({}), 5);
    assert_eq!(report.kind, "crash");
    assert!(report.frames.is_empty());
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn import_stores_the_report_deletes_the_file_and_makes_it_pending() {
    let data = scratch_dir("crash-import");
    let file = data.join("incoming.json");
    fs::write(&file, diagnostic().to_string()).unwrap();
    let stdout = import(&data, &file, 1_761_907_872).unwrap();
    assert_eq!(stdout, json!({"imported": "2025-10-31 10:51:12 UTC"}));
    assert!(!file.exists());
    let state = status(&data).unwrap();
    assert_eq!(state["pending"], 1);
    assert_eq!(state["latest"]["program"], "mac");
    assert_eq!(
        execute(data.clone(), &["acknowledge".into()]).unwrap(),
        json!({"acknowledged": 1})
    );
    assert_eq!(status(&data).unwrap()["pending"], 0);
    fs::remove_dir_all(data).ok();
}

#[test]
fn import_refuses_what_is_not_a_diagnostic_and_keeps_the_file() {
    let data = scratch_dir("crash-import-bad");
    let file = data.join("incoming.json");
    fs::write(&file, "not json").unwrap();
    assert!(import(&data, &file, 1).is_err());
    assert!(file.exists());
    fs::write(&file, vec![b' '; IMPORT_MAX_BYTES as usize + 1]).unwrap();
    assert!(import(&data, &file, 1).is_err());
    assert_eq!(status(&data).unwrap()["pending"], 0);
    fs::remove_dir_all(data).ok();
}

#[test]
fn an_unknown_subcommand_is_refused() {
    assert!(execute(std::env::temp_dir(), &["stat".into()]).is_err());
}
