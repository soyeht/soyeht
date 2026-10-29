use super::*;
use crate::testing::scratch_dir;
use std::fs;

fn input<'a>(data: &'a Path, redactor: &'a Redactor, lines: usize) -> Input<'a> {
    Input {
        product_version: "9.8.7",
        profile: "prod",
        engine: Presence::Running,
        supervisor: Presence::NotRunning,
        readiness: Readiness::Checked {
            system: "debian".into(),
            checks: vec![
                Check {
                    id: "git".into(),
                    required: true,
                    found: true,
                },
                Check {
                    id: "node".into(),
                    required: true,
                    found: false,
                },
                Check {
                    id: "wt".into(),
                    required: false,
                    found: false,
                },
            ],
        },
        data,
        logs: &["engine.log", "ptyd.log", "desktop.log"],
        lines,
        redactor,
    }
}

fn logs(data: &Path) -> std::path::PathBuf {
    let logs = logs_directory(data);
    fs::create_dir_all(&logs).unwrap();
    logs
}

#[test]
fn the_report_names_the_facts_and_keeps_no_path_name_or_token() {
    let data = scratch_dir("diagnostics-report");
    let logs = logs(&data);
    fs::write(
        logs.join("engine.log"),
        concat!(
            "{\"event\":\"store.open_failed\",\"path\":\"/Users/dana.example/Library/Work Files/db\",\"kind\":\"PermissionDenied\"}\n",
            "{\"event\":\"auth.failed\",\"token\":\"eyJhbGciOiJIUzI1NiJ9.payload123.signatureABC\",\"user\":\"dana.example\"}\n",
        ),
    )
    .unwrap();
    fs::write(logs.join("ptyd.log"), "").unwrap();
    let redactor = Redactor::new(Some(Path::new("/Users/dana.example")), Some("dana.example"));
    let text = report(&input(&data, &redactor, 10));
    for leaked in [
        "dana.example",
        "/Users",
        "Work Files",
        "eyJhbGci",
        "payload123",
    ] {
        assert!(!text.contains(leaked), "{leaked} leaked:\n{text}");
    }
    for wanted in [
        "- version: 9.8.7",
        "- profile: prod",
        "- engine: running",
        "- supervisor: not running",
        "- family: debian",
        "- git: found",
        "- node: MISSING (required)",
        "- wt: missing (optional)",
        "\"event\":\"store.open_failed\"",
        "\"kind\":\"PermissionDenied\"",
        "#### ptyd.log\n\n```\n(empty)\n```",
        "#### desktop.log\n\n```\n(no file)\n```",
    ] {
        assert!(text.contains(wanted), "{wanted} missing:\n{text}");
    }
    assert!(text.contains(std::env::consts::OS));
}

#[test]
fn only_the_last_lines_are_taken_and_the_rotated_file_fills_a_short_log() {
    let data = scratch_dir("diagnostics-tail");
    let logs = logs(&data);
    fs::write(logs.join("engine.log.1"), "old-1\nold-2\nold-3\n").unwrap();
    fs::write(logs.join("engine.log"), "new-1\nnew-2\n").unwrap();
    assert_eq!(
        tail(&logs.join("engine.log"), 4).unwrap(),
        ["old-2", "old-3", "new-1", "new-2"]
    );
    assert_eq!(tail(&logs.join("engine.log"), 1).unwrap(), ["new-2"]);
}

#[test]
fn a_long_log_is_read_from_its_end_and_a_long_line_is_cut() {
    let data = scratch_dir("diagnostics-long");
    let logs = logs(&data);
    let mut big = String::new();
    for index in 0..40_000 {
        big.push_str(&format!("line {index}\n"));
    }
    fs::write(logs.join("engine.log"), &big).unwrap();
    let lines = tail(&logs.join("engine.log"), 3).unwrap();
    assert_eq!(lines, ["line 39997", "line 39998", "line 39999"]);
    assert_eq!(
        clip(&"x".repeat(LINE_CHARS + 50)).chars().count(),
        LINE_CHARS + 3
    );
}

#[test]
fn an_engine_that_did_not_answer_is_said_so() {
    let data = scratch_dir("diagnostics-unavailable");
    let redactor = Redactor::default();
    let mut facts = input(&data, &redactor, 5);
    facts.readiness = Readiness::Unavailable;
    facts.engine = Presence::NotRunning;
    let text = report(&facts);
    assert!(text.contains("- engine: not running"));
    assert!(text.contains("The engine did not answer"));
}
