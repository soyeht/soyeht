//! The engine's atomic-save temporary file never shows up in a search, a
//! listing or a file event. A save that is in flight is simulated by planting
//! the temporary file next to its target, which is what a search racing the
//! autosave off the Core lock (ADR 0079) can walk over.

use super::{
    project_files::{enumerate_project_files, is_save_temporary},
    project_paths::ProjectRoot,
    project_watch_tests::Followed,
    test_fixture::EditorFixture,
};
use crate::dispatch::Outcome;
use notify::{EventKind, event::CreateKind};
use serde_json::{Value, json};
use std::{ffi::OsStr, fs};
use uuid::Uuid;

/// A name the atomic save of `soyeht_platform::fs` gives its temporary file.
fn save_temporary_name() -> String {
    format!(
        "{}{}",
        soyeht_platform::fs::TEMPORARY_PREFIX,
        Uuid::new_v4()
    )
}

/// What the autosave leaves beside `note.txt` while it writes: the new text
/// under the real writer's temporary name.
fn plant_save_in_flight(fixture: &EditorFixture) -> String {
    let name = save_temporary_name();
    fs::write(fixture.project().join(&name), "two one\n").unwrap();
    name
}

/// The relative paths the project enumeration finds in the fixture's project.
fn project_paths(fixture: &EditorFixture) -> Vec<String> {
    enumerate_project_files(&fixture.project())
        .unwrap()
        .into_iter()
        .map(|file| file.relative)
        .collect()
}

fn names(listing: &Value) -> Vec<&str> {
    listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect()
}

#[test]
fn the_writers_temporary_names_are_recognized_and_ordinary_names_are_not() {
    assert!(is_save_temporary(OsStr::new(&save_temporary_name())));
    for ordinary in ["note.txt", ".soyeht-saved", ".gitignore", "soyeht-write-x"] {
        assert!(!is_save_temporary(OsStr::new(ordinary)), "{ordinary}");
    }
}

#[tokio::test]
async fn a_save_in_flight_is_not_a_project_file() {
    let fixture = EditorFixture::new(&[("note.txt", "one\n")]).await;
    plant_save_in_flight(&fixture);
    assert_eq!(project_paths(&fixture), vec!["note.txt"]);
}

#[tokio::test]
async fn a_publish_in_flight_or_left_by_a_crash_is_not_a_project_file_or_a_tree_entry() {
    let mut fixture = EditorFixture::new(&[("note.txt", "one\n")]).await;
    let leftover = format!(
        "{}{}",
        soyeht_platform::fs::TEMPORARY_PREFIX,
        Uuid::new_v4()
    );
    fs::write(fixture.project().join(&leftover), "the whole draft\n").unwrap();
    assert_eq!(project_paths(&fixture), vec!["note.txt"]);
    let workspace_id = fixture.workspace_id.clone();
    let listing = fixture.call(
        "project.tree.list",
        json!({"workspaceId":workspace_id,"path":""}),
    );
    assert_eq!(names(&listing), vec!["note.txt"]);
}

#[tokio::test]
async fn searching_while_a_save_is_in_flight_finds_only_the_real_file() {
    let mut fixture = EditorFixture::new(&[("note.txt", "one\n")]).await;
    plant_save_in_flight(&fixture);
    // The temporary file holds "one" too, so a content search that walks it
    // finds two files; a path search for its name finds one.
    for (scope, query, expected) in [
        ("content", "one", vec!["note.txt"]),
        ("path", "soyeht", vec![]),
    ] {
        let request = json!({
            "jsonrpc":"2.0","id":1,"method":"file.search",
            "params":{
                "workspaceId":fixture.workspace_id,"scope":scope,"query":query,
                "caseSensitive":false,"limit":50,"idempotencyKey":Uuid::new_v4().to_string()
            }
        });
        let Outcome::Reply(reply) = fixture
            .core
            .dispatch_inline(&fixture.connection, request)
            .unwrap()
        else {
            panic!("expected a reply");
        };
        let found = reply["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(found, expected, "{scope}");
    }
}

#[tokio::test]
async fn the_project_panel_neither_lists_nor_accepts_a_save_in_flight() {
    let mut fixture = EditorFixture::new(&[("note.txt", "one\n")]).await;
    let workspace_id = fixture.workspace_id.clone();
    let before = ProjectRoot::open(&fixture.project())
        .unwrap()
        .fingerprint("")
        .unwrap();
    let name = plant_save_in_flight(&fixture);
    let listing = fixture.call(
        "project.tree.list",
        json!({"workspaceId":workspace_id,"path":"","showIgnored":true}),
    );
    assert_eq!(names(&listing), vec!["note.txt"]);
    // The folder looks unchanged, so polling it reports nothing.
    let after = ProjectRoot::open(&fixture.project())
        .unwrap()
        .fingerprint("")
        .unwrap();
    assert_eq!(before, after);
    assert!(
        fixture
            .try_call(
                "project.entry.create",
                json!({"workspaceId":workspace_id,"path":name,"kind":"file","idempotencyKey":Uuid::new_v4().to_string()}),
            )
            .is_err()
    );
}

#[tokio::test]
async fn a_save_in_flight_raises_no_file_event_and_costs_no_folder_read() {
    let fixture = EditorFixture::new(&[("note.txt", "one\n")]).await;
    let mut followed = Followed::attach(fixture);
    followed.watch(&[""]).await;
    let reads = followed.fixture.core.project_watches.reads();
    let name = plant_save_in_flight(&followed.fixture);
    followed.emit(EventKind::Create(CreateKind::File), &[&name]);
    assert!(followed.changes().await.is_empty());
    assert_eq!(followed.fixture.core.project_watches.reads(), reads);
}
