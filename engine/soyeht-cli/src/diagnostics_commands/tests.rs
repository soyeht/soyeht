use super::*;
use soyeht_protocol::{ReadinessItem, ReadinessKind, ReadinessSystem, profile::PROD};
use std::fs;

fn item(id: &str, required: bool, found: bool) -> ReadinessItem {
    ReadinessItem {
        id: id.into(),
        name: id.into(),
        kind: ReadinessKind::Tool,
        required,
        found,
        hint: Some("brew install thing".into()),
        note: None,
    }
}

#[test]
fn the_readiness_of_the_engine_becomes_checks_without_hints() {
    let result = EngineReadinessResult {
        system: ReadinessSystem::Fedora,
        items: vec![item("git", true, true), item("node", true, false)],
    };
    assert_eq!(
        checked(&result),
        Readiness::Checked {
            system: "fedora".into(),
            checks: vec![
                Check {
                    id: "git".into(),
                    required: true,
                    found: true
                },
                Check {
                    id: "node".into(),
                    required: true,
                    found: false
                },
            ],
        }
    );
}

#[test]
fn with_no_engine_the_report_still_has_the_logs_and_never_asks() {
    let data = std::env::temp_dir().join(format!("next-diagnostics-{}", std::process::id()));
    let logs = data.join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        logs.join("engine.log"),
        "{\"event\":\"engine.start_failed\",\"kind\":\"AddrInUse\",\"dir\":\"/Users/dana.example/Library/Work Files\"}\n",
    )
    .unwrap();
    let redactor = soyeht_platform::diagnostics::Redactor::new(
        Some(Path::new("/Users/dana.example")),
        Some("dana.example"),
        None,
    );
    let text = gather(&PROD, &data, 20, &redactor, |_| panic!("no engine to ask"));
    fs::remove_dir_all(&data).unwrap();
    println!("{text}");
    assert!(text.contains(&format!("- version: {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("- profile: prod"));
    assert!(text.contains("- engine: not running"));
    assert!(text.contains("- supervisor: not running"));
    assert!(text.contains("The engine did not answer"));
    assert!(text.contains("\"event\":\"engine.start_failed\""));
    assert!(text.contains("\"kind\":\"AddrInUse\""));
    for leaked in ["dana.example", "/Users", "Work Files"] {
        assert!(!text.contains(leaked), "{leaked} leaked:\n{text}");
    }
}

#[test]
fn the_lines_option_is_a_number() {
    assert!(
        execute(
            &PROD,
            PathBuf::from("/nonexistent/next"),
            &["--lines".into(), "many".into()]
        )
        .is_err()
    );
    assert!(
        execute(
            &PROD,
            PathBuf::from("/nonexistent/next"),
            &["--bytes".into(), "3".into()]
        )
        .is_err()
    );
}
