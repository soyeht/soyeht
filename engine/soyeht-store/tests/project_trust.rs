#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use soyeht_protocol::ProjectTrustChoice;
use soyeht_store::project_trust;
use support::Profile;

fn decide(
    store: &mut soyeht_store::database::Store,
    key: &str,
    root: &str,
    choice: ProjectTrustChoice,
) {
    store
        .mutate("test", key, "project-trust", |change| {
            project_trust::decide(change, root, &choice)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
}

#[test]
fn a_folder_with_no_answer_is_undecided() {
    let profile = Profile::new();
    let store = profile.store();
    assert_eq!(
        project_trust::decided(&store, "/work/project").unwrap(),
        None
    );
    assert!(project_trust::all(&store).unwrap().is_empty());
}

#[test]
fn an_answer_replaces_the_one_before_and_survives_reopening_the_store() {
    let profile = Profile::new();
    let mut store = profile.store();
    decide(
        &mut store,
        "one",
        "/work/project",
        ProjectTrustChoice::Trusted,
    );
    decide(
        &mut store,
        "two",
        "/work/other",
        ProjectTrustChoice::Restricted,
    );
    decide(
        &mut store,
        "three",
        "/work/project",
        ProjectTrustChoice::Restricted,
    );
    decide(
        &mut store,
        "four",
        "/work/project",
        ProjectTrustChoice::Trusted,
    );
    drop(store);
    let store = profile.store();
    assert_eq!(
        project_trust::decided(&store, "/work/project").unwrap(),
        Some(ProjectTrustChoice::Trusted)
    );
    assert_eq!(
        project_trust::all(&store).unwrap(),
        vec![
            ("/work/other".to_owned(), ProjectTrustChoice::Restricted),
            ("/work/project".to_owned(), ProjectTrustChoice::Trusted),
        ]
    );
}

#[test]
fn a_folder_that_cannot_be_a_key_is_refused() {
    let profile = Profile::new();
    let mut store = profile.store();
    let refused = store.mutate("test", "empty", "project-trust", |change| {
        project_trust::decide(change, "", &ProjectTrustChoice::Trusted)?;
        Ok((json!({}), Vec::new()))
    });
    assert!(refused.is_err());
    assert!(project_trust::all(&store).unwrap().is_empty());
}

#[test]
fn a_database_of_schema_32_gains_the_table_and_every_folder_starts_undecided() {
    let profile = Profile::new();
    let mut store = profile.store();
    decide(
        &mut store,
        "one",
        "/work/project",
        ProjectTrustChoice::Trusted,
    );
    drop(store);
    let connection = profile.connection();
    connection
        .execute_batch("DROP TABLE project_trust; PRAGMA user_version = 32;")
        .unwrap();
    drop(connection);

    let store = profile.store();
    let version: u32 = store
        .connection()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, soyeht_store::database::SCHEMA_VERSION);
    assert_eq!(
        project_trust::decided(&store, "/work/project").unwrap(),
        None
    );
    assert!(project_trust::all(&store).unwrap().is_empty());
}
