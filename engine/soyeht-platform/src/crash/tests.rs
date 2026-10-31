use super::{
    capture::{message_kind, source_path},
    report::*,
    store::*,
    *,
};
use crate::{diagnostics::Redactor, testing::scratch_dir};
use std::{fs, path::Path};

fn sample(time: u64, fatal: bool) -> Report {
    Report {
        version: "1.2.3".into(),
        system: "macos aarch64".into(),
        program: Program::Engine,
        time,
        pid: 77,
        thread: "main".into(),
        fatal,
        kind: "unwrap_none".into(),
        location: "soyeht-engine/src/launch.rs:12:5".into(),
        frames: vec!["soyeht-engine+0x1a2b3c".into(), "libsystem+0x10".into()],
        log: vec![r#"{"event":"engine.ready"}"#.into()],
    }
}

#[test]
fn a_report_is_read_back_as_written() {
    let report = sample(1_761_907_872, true);
    assert_eq!(Report::parse(&report.to_text()), Some(report));
}

#[test]
fn a_text_that_is_not_a_report_is_refused() {
    assert_eq!(Report::parse("hello"), None);
    assert_eq!(
        Report::parse("soyeht-crash-report 2\nprogram: engine\n"),
        None
    );
    assert_eq!(
        Report::parse("soyeht-crash-report 1\nprogram: toaster\n"),
        None
    );
    assert_eq!(Report::parse(&"x".repeat(MAX_REPORT_BYTES * 3)), None);
}

#[test]
fn a_report_stays_within_its_bounds() {
    let mut report = sample(1, true);
    report.frames = (0..500).map(|n| format!("module+{n:#x}")).collect();
    report.log = (0..500)
        .map(|n| format!("{{\"event\":\"line.{n}\"}}"))
        .collect();
    report.location = "l".repeat(5_000);
    let text = report.to_text();
    assert!(text.len() <= MAX_REPORT_BYTES);
    let read = Report::parse(&text).unwrap();
    assert_eq!(read.frames.len(), MAX_FRAMES);
    assert_eq!(read.log.len(), MAX_LOG_LINES);
    assert!(read.location.chars().count() <= 160);
}

#[test]
fn a_value_cannot_start_a_section_or_a_line() {
    let mut report = sample(1, true);
    report.kind = "a\nfatal: no\nlog:".into();
    let read = Report::parse(&report.to_text()).unwrap();
    assert!(read.fatal);
    assert!(!read.kind.contains('\n'));
}

#[test]
fn the_time_is_written_in_utc() {
    assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
    assert_eq!(utc(1_761_907_872), "2025-10-31 10:51:12 UTC");
    assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC");
}

#[test]
fn a_source_path_carries_no_folder_of_the_person() {
    assert_eq!(
        source_path("soyeht-engine/src/launch.rs"),
        "soyeht-engine/src/launch.rs"
    );
    assert_eq!(
        source_path("../soyeht-platform/src/x.rs"),
        "soyeht-platform/src/x.rs"
    );
    assert_eq!(
        source_path(
            "/Users/dana/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.40.0/src/x.rs"
        ),
        "tokio-1.40.0/src/x.rs"
    );
    assert_eq!(
        source_path(
            "C:\\Users\\dana\\.cargo\\registry\\src\\index.crates.io-1\\tokio-1.40.0\\src\\x.rs"
        ),
        "tokio-1.40.0/src/x.rs"
    );
    assert_eq!(
        source_path("/rustc/8bab26f4f68e0e26f0bb7960be334d5b520ea452/library/core/src/option.rs"),
        "library/core/src/option.rs"
    );
    let other = source_path("/Users/dana/Projects/secret-app/src/lib.rs");
    assert!(!other.contains("dana") && !other.contains("Projects"));
    assert!(!source_path("C:/Users/dana/work/app/src/lib.rs").contains("dana"));
}

#[test]
fn a_panic_message_is_reduced_to_a_label() {
    assert_eq!(
        message_kind("called `Option::unwrap()` on a `None` value"),
        "unwrap_none"
    );
    assert_eq!(
        message_kind("index out of bounds: the len is 3 but the index is 5"),
        "index_out_of_bounds"
    );
    assert_eq!(message_kind("attempt to divide by zero"), "arithmetic");
    assert_eq!(message_kind("secret text of the person"), "explicit");
}

#[test]
fn frames_are_module_and_offset() {
    if cfg!(any(windows, all(unix, not(target_env = "musl")))) {
        let frames = frames::capture(MAX_FRAMES);
        assert!(!frames.is_empty());
        assert!(
            frames
                .iter()
                .all(|frame| frame.contains("+0x") && !frame.contains('/'))
        );
    }
}

fn names_of(data: &Path) -> Vec<String> {
    list(data)
        .unwrap()
        .into_iter()
        .map(|stored| stored.name)
        .collect()
}

#[test]
fn the_folder_keeps_the_newest_reports_only() {
    let data = scratch_dir("crash-bound");
    for time in 1..=(MAX_REPORTS as u64 + 4) {
        write(&data, &sample(time, true)).unwrap();
    }
    let names = names_of(&data);
    assert_eq!(names.len(), MAX_REPORTS);
    assert!(names[0].starts_with("crash-000000000005-"));
    assert!(names[MAX_REPORTS - 1].starts_with("crash-000000000012-"));
    fs::remove_dir_all(data).ok();
}

#[test]
fn a_report_is_pending_until_it_is_acknowledged() {
    let data = scratch_dir("crash-pending");
    assert!(pending(&data).unwrap().is_empty());
    write(&data, &sample(10, true)).unwrap();
    write(&data, &sample(11, false)).unwrap();
    assert_eq!(
        pending(&data).unwrap().len(),
        1,
        "a panic that ended a task is not a closing"
    );
    assert_eq!(acknowledge(&data, &[]).unwrap(), 1);
    assert!(pending(&data).unwrap().is_empty());
    assert_eq!(acknowledge(&data, &[]).unwrap(), 0);
    assert_eq!(
        names_of(&data).len(),
        2,
        "the reports stay for the diagnostics"
    );
    write(&data, &sample(12, true)).unwrap();
    assert_eq!(pending(&data).unwrap().len(), 1);
    fs::remove_dir_all(data).ok();
}

#[test]
fn a_file_that_is_not_a_report_is_left_out() {
    let data = scratch_dir("crash-foreign");
    write(&data, &sample(5, true)).unwrap();
    let folder = directory(&data);
    fs::write(
        folder.join("crash-000000000009-engine-1.txt"),
        "not a report",
    )
    .unwrap();
    fs::write(folder.join("notes.txt"), "x").unwrap();
    assert_eq!(list(&data).unwrap().len(), 1);
    fs::remove_dir_all(data).ok();
}

#[test]
fn the_diagnostics_text_removes_the_home_folder_and_redacts_the_log() {
    let data = scratch_dir("crash-markdown");
    let mut report = sample(1_761_907_872, true);
    report.location = "/Users/dana/Projects/x.rs:1:1".into();
    report.log = vec![r#"{"event":"x","path":"/Users/dana/Projects/secret.txt"}"#.into()];
    write(&data, &report).unwrap();
    let redactor = Redactor::new(Some(Path::new("/Users/dana")), Some("dana"), None);
    let text = markdown(&list(&data).unwrap(), &redactor).join("\n");
    assert!(text.contains("#### engine · 2025-10-31 10:51:12 UTC · closed"));
    assert!(text.contains("program: engine"));
    assert!(
        !text.contains("dana") && !text.contains("secret.txt"),
        "{text}"
    );
    fs::remove_dir_all(data).ok();
}

#[test]
fn acknowledging_what_was_shown_leaves_a_later_or_earlier_named_report_pending() {
    let data = scratch_dir("crash-ack-names");
    let mut first = sample(100, true);
    first.program = Program::Supervisor;
    write(&data, &first).unwrap();
    let shown = pending(&data).unwrap();
    // Two more arrive: one in the same second whose name sorts BEFORE the shown one (a program
    // name that sorts earlier), one after a clock that stepped back.
    write(&data, &sample(100, true)).unwrap();
    write(&data, &sample(50, true)).unwrap();
    let names: Vec<String> = shown.iter().map(|stored| stored.name.clone()).collect();
    assert_eq!(acknowledge(&data, &names).unwrap(), 1);
    assert_eq!(pending(&data).unwrap().len(), 2);
    assert_eq!(acknowledge(&data, &[]).unwrap(), 2);
    assert!(pending(&data).unwrap().is_empty());
    fs::remove_dir_all(data).ok();
}

#[test]
fn the_list_of_seen_reports_forgets_the_reports_that_are_gone() {
    let data = scratch_dir("crash-ack-forget");
    for time in 1..=(MAX_REPORTS as u64 + 3) {
        write(&data, &sample(time, true)).unwrap();
        acknowledge(&data, &[]).unwrap();
    }
    let kept = fs::read_to_string(directory(&data).join("acknowledged")).unwrap();
    assert_eq!(kept.lines().count(), MAX_REPORTS);
    fs::remove_dir_all(data).ok();
}

#[test]
fn reports_written_at_the_same_moment_by_threads_never_replace_each_other() {
    let data = scratch_dir("crash-same-second");
    for round in 0..20 {
        let folder = data.join(round.to_string());
        let writers: Vec<_> = (0..4)
            .map(|_| {
                let folder = folder.clone();
                std::thread::spawn(move || {
                    let mut report = sample(7, true);
                    report.kind = format!("{:?}", std::thread::current().id());
                    write(&folder, &report).unwrap();
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        assert_eq!(list(&folder).unwrap().len(), 4, "round {round}");
    }
    fs::remove_dir_all(data).ok();
}
