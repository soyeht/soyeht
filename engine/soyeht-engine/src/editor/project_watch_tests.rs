//! The project panel follows disk changes from file events, and an idle
//! workspace costs the engine no folder reads.

use super::test_fixture::EditorFixture;
use serde_json::{Value, json};
use std::{fs, time::Duration};

/// Simulated time per engine turn: past the quiet period, so a burst that has
/// arrived is read on the next turn.
const TURN_MS: i64 = 100;

/// Waits `wait_ms` of real time for events, then runs one engine turn
/// `advance_ms` of simulated time later.
async fn turn(fixture: &mut EditorFixture, wait_ms: u64, advance_ms: i64) {
    tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    let now = fixture.core.now_ms() + advance_ms;
    fixture.at(now);
    fixture.core.advance_project_tree().unwrap();
}

/// Turns the engine until it reports a change, waiting for the operating
/// system to deliver the events; empty after five seconds.
pub(super) async fn next_change(fixture: &mut EditorFixture) -> Vec<Value> {
    for _ in 0..100 {
        turn(fixture, 50, TURN_MS).await;
        let found = fixture.notifications().await;
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// Lets events from the fixture's own setup arrive and be read.
async fn settle(fixture: &mut EditorFixture) {
    for _ in 0..10 {
        turn(fixture, 50, TURN_MS).await;
    }
    fixture.notifications().await;
}

fn changed(workspace_id: &str, paths: &[&str]) -> Vec<Value> {
    vec![json!({"jsonrpc":"2.0","method":"project.tree_changed",
        "params":{"workspaceId":workspace_id,"paths":paths}})]
}

async fn watching(paths: &[&str], polling: bool) -> EditorFixture {
    let mut fixture = EditorFixture::new(&[("src/lib.rs", "lib\n"), ("README.md", "r\n")]).await;
    fixture.core.project_watches.force_polling = polling;
    let workspace_id = fixture.workspace_id.clone();
    fixture.call(
        "project.tree.watch",
        json!({"workspaceId":workspace_id,"paths":paths}),
    );
    fixture.notifications().await;
    fixture
}

#[tokio::test]
async fn create_rename_and_delete_on_disk_are_reported_from_events() {
    let mut fixture = watching(&["", "src"], false).await;
    let workspace_id = fixture.workspace_id.clone();
    assert!(fixture.core.project_watches.uses_events(&workspace_id));
    settle(&mut fixture).await;
    let src = fixture.project().join("src");

    fs::write(src.join("new.rs"), "").unwrap();
    assert_eq!(
        next_change(&mut fixture).await,
        changed(&workspace_id, &["src"])
    );

    fs::rename(src.join("new.rs"), src.join("renamed.rs")).unwrap();
    assert_eq!(
        next_change(&mut fixture).await,
        changed(&workspace_id, &["src"])
    );

    fs::remove_file(src.join("renamed.rs")).unwrap();
    assert_eq!(
        next_change(&mut fixture).await,
        changed(&workspace_id, &["src"])
    );

    fs::create_dir(fixture.project().join("docs")).unwrap();
    assert_eq!(
        next_change(&mut fixture).await,
        changed(&workspace_id, &[""])
    );

    // Writing a file in place leaves every listing alone: nothing is sent.
    fs::write(fixture.project().join("README.md"), "changed\n").unwrap();
    assert!(next_change(&mut fixture).await.is_empty());
}

#[tokio::test]
async fn a_burst_of_changes_becomes_one_notification() {
    let mut fixture = watching(&["src"], false).await;
    let workspace_id = fixture.workspace_id.clone();
    settle(&mut fixture).await;
    for index in 0..50 {
        fs::write(fixture.project().join(format!("src/f{index}.rs")), "").unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        next_change(&mut fixture).await,
        changed(&workspace_id, &["src"])
    );
    settle(&mut fixture).await;
    assert!(fixture.notifications().await.is_empty());
}

#[tokio::test]
async fn an_idle_workspace_costs_no_folder_reads() {
    let mut fixture = watching(&["", "src"], false).await;
    settle(&mut fixture).await;
    let reads = fixture.core.project_watches.reads();
    for _ in 0..20 {
        turn(&mut fixture, 25, 1_000).await;
    }
    assert_eq!(fixture.core.project_watches.reads(), reads);
    assert!(fixture.notifications().await.is_empty());
}

#[tokio::test]
async fn without_events_the_watched_folders_are_polled_once_a_second() {
    let mut fixture = watching(&["src"], true).await;
    let workspace_id = fixture.workspace_id.clone();
    assert!(!fixture.core.project_watches.uses_events(&workspace_id));
    let start = fixture.core.now_ms();
    fixture.core.advance_project_tree().unwrap();
    let reads = fixture.core.project_watches.reads();
    fixture.at(start + 500);
    fixture.core.advance_project_tree().unwrap();
    assert_eq!(fixture.core.project_watches.reads(), reads);

    fs::write(fixture.project().join("src/new.rs"), "").unwrap();
    fixture.at(start + 1_000);
    fixture.core.advance_project_tree().unwrap();
    assert_eq!(
        fixture.notifications().await,
        changed(&workspace_id, &["src"])
    );
}
