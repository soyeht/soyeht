//! A watched file whose workspace folder disappears used to be re-checked and
//! logged on every 250 ms tick, which filled the engine log without bound.

use super::{Effect, test_fixture::EditorFixture};
use serde_json::Value;
use std::fs;

const TICK_MS: i64 = 250;
const TEN_MINUTES_MS: i64 = 600_000;

fn events<'a>(records: &'a [Value], name: &str) -> Vec<&'a Value> {
    records
        .iter()
        .filter(|record| record["event"] == name)
        .collect()
}

fn open_main(fixture: &mut EditorFixture) -> String {
    fixture.open("main.rs", false)["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_watch_on_a_vanished_file_backs_off_and_logs_few_times() {
    let mut fixture = EditorFixture::new(&[("main.rs", "fn main() {}\n")]).await;
    let file_id = open_main(&mut fixture);
    fs::remove_dir_all(fixture.project()).unwrap();

    let mut records = Vec::new();
    let mut now = 1_000;
    while now < 1_000 + TEN_MINUTES_MS {
        fixture.at(now);
        records.extend(fixture.core.advance_editor_effects());
        now += TICK_MS;
    }

    // Ten minutes of 250 ms ticks is 2400 checks. Backing off keeps the real
    // attempts near a dozen, and the log to the power-of-two attempts only.
    let attempts = fixture.core.editor.effect_attempts(&file_id, Effect::Watch);
    assert!((10..=25).contains(&attempts), "attempts: {attempts}");
    let failed = events(&records, "engine.editor_effect_failed");
    assert!(failed.len() <= 6, "logged {} failures", failed.len());
    assert_eq!(failed.len(), records.len());
    let first = failed[0];
    assert_eq!(first["action"], "watch");
    assert_eq!(first["fileId"], file_id.as_str());
    assert_eq!(first["cause"], "io:NotFound");
    assert_eq!(first["attempts"], 1);
    assert_eq!(first["retryInMs"], 500);
}

#[tokio::test]
async fn a_watch_that_recovers_logs_one_recovery_and_resets() {
    let mut fixture = EditorFixture::new(&[("main.rs", "fn main() {}\n")]).await;
    let file_id = open_main(&mut fixture);
    let project = fixture.project();
    fs::rename(&project, fixture.root.join("moved")).unwrap();

    let mut now = 1_000;
    for _ in 0..20 {
        fixture.at(now);
        fixture.core.advance_editor_effects();
        now += TICK_MS;
    }
    assert!(fixture.core.editor.effect_attempts(&file_id, Effect::Watch) > 0);

    fs::rename(fixture.root.join("moved"), &project).unwrap();
    let mut records = Vec::new();
    for _ in 0..400 {
        now += TICK_MS;
        fixture.at(now);
        records.extend(fixture.core.advance_editor_effects());
    }

    let recovered = events(&records, "engine.editor_effect_recovered");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0]["action"], "watch");
    assert_eq!(
        fixture.core.editor.effect_attempts(&file_id, Effect::Watch),
        0
    );
}
