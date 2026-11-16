#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_protocol::{Avatar, AvatarBinding, AvatarHearing, AvatarVoice, Workspace};
use soyeht_store::{avatars, database::Error, database::Store};
use support::{Profile, commit};

fn workspace(id: &str) -> Workspace {
    Workspace {
        workspace_id: id.into(),
        name: id.into(),
        path: format!("/work/{id}"),
        layout: serde_json::json!({}),
    }
}

fn avatar(id: &str, name: &str) -> Avatar {
    Avatar {
        avatar_id: id.into(),
        name: name.into(),
        kind: "uploaded".into(),
        source_hash: "a".repeat(64),
        image_provider: Some("a-provider".into()),
        image_model: Some("a-model".into()),
        voice: Some(AvatarVoice {
            provider_id: "system".into(),
            voice_id: "default".into(),
        }),
        created_at: "2026-11-15T10:00:00Z".into(),
    }
}

fn binding(workspace: &str, avatar: &str, revision: u32) -> AvatarBinding {
    AvatarBinding {
        workspace_id: workspace.into(),
        avatar_id: avatar.into(),
        harness: "claude-code".into(),
        model: Some("a-model".into()),
        effort: None,
        hearing: Some(AvatarHearing {
            provider_id: "system".into(),
            language: Some("pt-BR".into()),
        }),
        voice: None,
        persona: "Speak briefly.".into(),
        revision: revision.to_string(),
    }
}

fn add(store: &mut Store, key: &str, avatar: &Avatar) {
    commit(store, key, |tx| avatars::insert_in(tx, avatar));
}

fn open_workspace(store: &mut Store, id: &str) {
    commit(store, id, |tx| tx.insert_workspace(&workspace(id)));
}

fn bind(store: &mut Store, key: &str, binding: &AvatarBinding) -> Result<(), Error> {
    store
        .mutate("test", key, key, |tx| {
            avatars::bind_in(tx, binding)?;
            Ok((serde_json::json!({}), vec![]))
        })
        .map(drop)
}

/// A store with the avatar `a1` bound, at revision 1, to the open workspace `w1`.
fn store_with_a_binding(profile: &Profile) -> Store {
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "One"));
    open_workspace(&mut store, "w1");
    bind(&mut store, "first", &binding("w1", "a1", 1)).unwrap();
    store
}

fn assert_stale(store: &mut Store, key: &str, binding: &AvatarBinding) {
    assert!(matches!(
        bind(store, key, binding),
        Err(Error::StaleAvatarBinding)
    ));
}

#[test]
fn an_avatar_is_kept_as_it_was_made_and_the_library_lists_oldest_first() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "one", &avatar("b", "Second by id, first made"));
    add(&mut store, "two", &avatar("a", "First by id, second made"));
    commit(&mut store, "rename", |tx| {
        avatars::rename_in(tx, "a", "Mentor").map(drop)
    });
    drop(store);
    let store = profile.store();
    let listed = avatars::list(&store).unwrap();
    let names: Vec<_> = listed.iter().map(|avatar| avatar.name.as_str()).collect();
    assert_eq!(names, ["Second by id, first made", "Mentor"]);
    assert_eq!(listed[0].image_provider.as_deref(), Some("a-provider"));
    assert_eq!(listed[0].image_model.as_deref(), Some("a-model"));
    assert_eq!(listed[0].voice.as_ref().unwrap().voice_id, "default");
    assert_eq!(avatars::count(&store).unwrap(), 2);
    assert_eq!(avatars::ids(&store).unwrap(), ["b", "a"]);
    assert_eq!(avatars::get(&store, "missing").unwrap(), None);
}

#[test]
fn a_kind_nobody_defined_reads_back_as_it_was_stored() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "Sabio"));
    add(&mut store, "a2", &avatar("a2", "Other"));
    store
        .connection()
        .execute("UPDATE avatars SET kind='hologram' WHERE id='a1'", [])
        .unwrap();
    let listed = avatars::list(&store).unwrap();
    assert_eq!(listed[0].kind, "hologram");
    assert_eq!(listed[1].kind, "uploaded");
}

#[test]
fn a_binding_is_replaced_one_revision_at_a_time() {
    let profile = Profile::new();
    let mut store = store_with_a_binding(&profile);
    add(&mut store, "a2", &avatar("a2", "Two"));
    let mut changed = binding("w1", "a2", 2);
    changed.persona = String::new();
    changed.hearing = None;
    bind(&mut store, "second", &changed).unwrap();
    let kept = avatars::binding(&store, "w1").unwrap().unwrap();
    assert_eq!(kept.avatar_id, "a2");
    assert_eq!((kept.persona.as_str(), kept.hearing), ("", None));
    assert_eq!(kept.revision, "2");
}

#[test]
fn a_binding_written_from_an_old_copy_is_refused_and_changes_nothing() {
    let profile = Profile::new();
    let mut store = store_with_a_binding(&profile);
    let mut newer = binding("w1", "a1", 2);
    newer.persona = "v2".into();
    bind(&mut store, "second", &newer).unwrap();
    let mut stale = binding("w1", "a1", 2);
    stale.persona = "v1".into();
    assert_stale(&mut store, "stale", &stale);
    // A revision that skips one, and a first binding that is not revision 1, are stale too.
    assert_stale(&mut store, "skip", &binding("w1", "a1", 4));
    open_workspace(&mut store, "w2");
    assert_stale(&mut store, "late", &binding("w2", "a1", 2));
    assert_eq!(
        avatars::binding(&store, "w1").unwrap().unwrap().persona,
        "v2"
    );
    assert_eq!(avatars::binding(&store, "w2").unwrap(), None);
}

#[test]
fn closing_a_workspace_takes_its_binding_and_a_bound_avatar_is_not_deleted() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "One"));
    open_workspace(&mut store, "w1");
    open_workspace(&mut store, "w2");
    bind(&mut store, "b1", &binding("w1", "a1", 1)).unwrap();
    bind(&mut store, "b2", &binding("w2", "a1", 1)).unwrap();
    let listed: Vec<_> = avatars::bindings(&store)
        .unwrap()
        .into_iter()
        .map(|binding| binding.workspace_id)
        .collect();
    assert_eq!(listed, ["w1", "w2"]);
    let refused = store.mutate("test", "delete-bound", "x", |tx| {
        avatars::delete_in(tx, "a1")?;
        Ok((serde_json::json!({}), vec![]))
    });
    assert!(matches!(refused, Err(Error::AvatarInUse)));
    assert!(avatars::get(&store, "a1").unwrap().is_some());
    commit(&mut store, "close-1", |tx| tx.close_workspace("w1"));
    assert_eq!(avatars::binding(&store, "w1").unwrap(), None);
    let still: Vec<_> = avatars::bindings(&store)
        .unwrap()
        .into_iter()
        .map(|binding| binding.workspace_id)
        .collect();
    assert_eq!(still, ["w2"]);
    commit(&mut store, "close-2", |tx| tx.close_workspace("w2"));
    let mut removed = false;
    commit(&mut store, "delete", |tx| {
        removed = avatars::delete_in(tx, "a1")?;
        Ok(())
    });
    assert!(removed);
    assert_eq!(avatars::get(&store, "a1").unwrap(), None);
    commit(&mut store, "delete-again", |tx| {
        removed = avatars::delete_in(tx, "a1")?;
        Ok(())
    });
    assert!(!removed);
}

#[test]
fn a_blank_name_or_a_short_hash_is_not_an_avatar() {
    let profile = Profile::new();
    let mut store = profile.store();
    let mut short = avatar("a1", "Sabio");
    short.source_hash = "abc".into();
    for (key, record) in [("blank", avatar("a2", "")), ("short", short)] {
        let refused = store.mutate("test", key, key, |tx| {
            avatars::insert_in(tx, &record)?;
            Ok((serde_json::json!({}), vec![]))
        });
        assert!(refused.is_err());
    }
    assert_eq!(avatars::count(&store).unwrap(), 0);
}

#[test]
fn a_database_of_schema_37_gains_the_avatar_tables_and_keeps_what_it_had() {
    let profile = Profile::new();
    let mut store = profile.store();
    open_workspace(&mut store, "w1");
    drop(store);
    let connection = profile.connection();
    connection
        .execute_batch(
            "DROP TABLE workspace_avatars; DROP TABLE avatars; PRAGMA user_version = 37;",
        )
        .unwrap();
    drop(connection);
    let mut store = profile.store();
    let version = store
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
        .unwrap();
    assert_eq!(version, soyeht_store::database::SCHEMA_VERSION);
    assert!(avatars::list(&store).unwrap().is_empty());
    assert!(avatars::bindings(&store).unwrap().is_empty());
    add(&mut store, "a1", &avatar("a1", "Sabio"));
    bind(&mut store, "bind", &binding("w1", "a1", 1)).unwrap();
    assert_eq!(avatars::bindings(&store).unwrap().len(), 1);
}

#[test]
fn the_migration_runs_again_on_a_database_whose_version_was_set_back_and_keeps_the_avatars() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "Sabio"));
    drop(store);
    profile
        .connection()
        .execute_batch("PRAGMA user_version = 37;")
        .unwrap();
    let store = profile.store();
    assert_eq!(avatars::count(&store).unwrap(), 1);
}

#[test]
fn the_persona_of_a_workspace_is_its_avatars_name_and_the_bindings_text() {
    let profile = Profile::new();
    let mut store = store_with_a_binding(&profile);
    let persona = avatars::persona(&store, "w1").unwrap().unwrap();
    assert_eq!(persona.name, "One");
    assert_eq!(persona.text, "Speak briefly.");
    // A rename of the avatar and a new binding are what the next read finds.
    commit(&mut store, "rename", |tx| {
        avatars::rename_in(tx, "a1", "Mentor").map(drop)
    });
    let mut newer = binding("w1", "a1", 2);
    newer.persona = String::new();
    bind(&mut store, "second", &newer).unwrap();
    let persona = avatars::persona(&store, "w1").unwrap().unwrap();
    assert_eq!(
        (persona.name.as_str(), persona.text.as_str()),
        ("Mentor", "")
    );
    // A workspace with no avatar has none.
    open_workspace(&mut store, "w2");
    assert_eq!(avatars::persona(&store, "w2").unwrap(), None);
}
