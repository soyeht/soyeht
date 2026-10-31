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
    let report = metrickit::report(&diagnostic(), 1_761_907_872).unwrap();
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
    let report = metrickit::report(&json!({"diagnosticMetaData": {}}), 5).unwrap();
    assert_eq!(report.kind, "crash");
    assert!(report.frames.is_empty());
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
}

/// A diagnostic where the app drops it: `incoming-*` in `crashes/`.
fn drop_diagnostic(data: &Path, text: &str) -> PathBuf {
    let folder = store::directory(data);
    fs::create_dir_all(&folder).unwrap();
    let file = folder.join("incoming-1.json");
    fs::write(&file, text).unwrap();
    file
}

#[test]
fn import_stores_the_report_deletes_the_file_and_makes_it_pending() {
    let data = scratch_dir("crash-import");
    let file = drop_diagnostic(&data, &diagnostic().to_string());
    let stdout = import(&data, &file, 1_761_907_872).unwrap();
    assert_eq!(stdout, json!({"imported": "2025-10-31 10:51:12 UTC"}));
    assert!(!file.exists());
    let state = status(&data).unwrap();
    assert_eq!(state["pending"], 1);
    assert_eq!(state["latest"]["program"], "mac");
    let shown = state["reports"][0].as_str().unwrap().to_owned();
    assert_eq!(
        execute(data.clone(), &["acknowledge".into(), shown]).unwrap(),
        json!({"acknowledged": 1})
    );
    assert_eq!(status(&data).unwrap()["pending"], 0);
    fs::remove_dir_all(data).ok();
}

#[test]
fn acknowledge_marks_what_was_shown_and_a_later_report_stays_pending() {
    let data = scratch_dir("crash-ack-shown");
    for (n, time) in [(1, 10), (2, 11)] {
        let file = store::directory(&data).join(format!("incoming-{n}.json"));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, diagnostic().to_string()).unwrap();
        import(&data, &file, time).unwrap();
    }
    let shown = status(&data).unwrap()["reports"][0]
        .as_str()
        .unwrap()
        .to_owned();
    execute(data.clone(), &["acknowledge".into(), shown]).unwrap();
    assert_eq!(status(&data).unwrap()["pending"], 1);
    execute(data.clone(), &["acknowledge".into()]).unwrap();
    assert_eq!(status(&data).unwrap()["pending"], 0);
    fs::remove_dir_all(data).ok();
}

#[test]
fn import_refuses_what_is_not_a_diagnostic_and_keeps_the_file() {
    let data = scratch_dir("crash-import-bad");
    for text in ["not json", "{}", r#"{"hello": 1}"#] {
        let file = drop_diagnostic(&data, text);
        assert!(import(&data, &file, 1).is_err(), "{text}");
        assert!(file.exists());
    }
    let file = drop_diagnostic(&data, "");
    fs::write(&file, vec![b' '; IMPORT_MAX_BYTES as usize + 1]).unwrap();
    assert!(import(&data, &file, 1).is_err());
    assert_eq!(status(&data).unwrap()["pending"], 0);
    fs::remove_dir_all(data).ok();
}

#[test]
fn import_deletes_only_what_the_app_dropped() {
    let data = scratch_dir("crash-import-only");
    drop_diagnostic(&data, "");
    let valid = diagnostic().to_string();
    // Outside crashes/, with the right name; inside it, with another name; a link to a good file.
    let outside = data.join("incoming-2.json");
    fs::write(&outside, &valid).unwrap();
    let misnamed = store::directory(&data).join("notes.json");
    fs::write(&misnamed, &valid).unwrap();
    let target = data.join("target.json");
    fs::write(&target, &valid).unwrap();
    let link = store::directory(&data).join("incoming-3.json");
    soyeht_platform::fs::symlink(&target, &link).unwrap();
    for file in [&outside, &misnamed, &link, &data.join("nope")] {
        assert!(import(&data, file, 1).is_err(), "{}", file.display());
    }
    assert!(outside.exists() && misnamed.exists() && target.exists());
    assert_eq!(status(&data).unwrap()["pending"], 0);
    fs::remove_dir_all(data).ok();
}

#[test]
fn an_unknown_subcommand_is_refused() {
    assert!(execute(std::env::temp_dir(), &["stat".into()]).is_err());
}
