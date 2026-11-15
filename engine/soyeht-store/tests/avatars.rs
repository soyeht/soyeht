#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_protocol::{
    Avatar, AvatarBinding, AvatarBuildState, AvatarHearing, AvatarKind, AvatarVoice, Workspace,
};
use soyeht_store::{avatars, database::Store};
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
        kind: AvatarKind::Uploaded,
        source_hash: "a".repeat(64),
        build_state: AvatarBuildState::Ready,
        failure_kind: None,
        package_hash: None,
        voice: Some(AvatarVoice {
            provider_id: "system".into(),
            voice_id: "default".into(),
        }),
        created_at: "2026-11-15T10:00:00Z".into(),
    }
}

fn binding(workspace: &str, avatar: &str) -> AvatarBinding {
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
        voice_on: vec![],
    }
}

fn add(store: &mut Store, key: &str, avatar: &Avatar) {
    commit(store, key, |tx| avatars::insert_in(tx, avatar));
}

fn open_workspace(store: &mut Store, id: &str) {
    commit(store, id, |tx| tx.insert_workspace(&workspace(id)));
}

#[test]
fn a_new_avatar_has_nothing_built_whatever_the_record_said() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "one", &avatar("a1", "Sabio"));
    let stored = avatars::get(&store, "a1").unwrap().unwrap();
    assert_eq!(stored.build_state, AvatarBuildState::None);
    assert_eq!(stored.failure_kind, None);
    assert_eq!(stored.package_hash, None);
    assert_eq!(stored.voice.unwrap().voice_id, "default");
    assert_eq!(avatars::count(&store).unwrap(), 1);
    assert_eq!(avatars::get(&store, "missing").unwrap(), None);
}

#[test]
fn the_library_lists_oldest_first_and_survives_reopening_the_store() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "one", &avatar("b", "Second by id, first made"));
    add(&mut store, "two", &avatar("a", "First by id, second made"));
    commit(&mut store, "rename", |tx| {
        avatars::rename_in(tx, "a", "Mentor").map(drop)
    });
    drop(store);
    let store = profile.store();
    let names: Vec<_> = avatars::list(&store)
        .unwrap()
        .into_iter()
        .map(|avatar| avatar.name)
        .collect();
    assert_eq!(names, ["Second by id, first made", "Mentor"]);
}

#[test]
fn a_build_starts_only_from_nothing_built_or_a_failure() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "one", &avatar("a1", "Sabio"));
    let started = |store: &mut Store, key: &str| {
        let mut answer = false;
        commit(store, key, |tx| {
            answer = avatars::start_build_in(tx, "a1")?;
            Ok(())
        });
        answer
    };
    assert!(started(&mut store, "first"));
    assert_eq!(
        avatars::get(&store, "a1").unwrap().unwrap().build_state,
        AvatarBuildState::Building
    );
    assert!(!started(&mut store, "second"));
    store
        .connection()
        .execute(
            "UPDATE avatars SET build_state='failed', failure_kind='frames' WHERE id='a1'",
            [],
        )
        .unwrap();
    let failed = avatars::get(&store, "a1").unwrap().unwrap();
    assert_eq!(
        failed.failure_kind.map(|kind| format!("{kind:?}")),
        Some("Frames".into())
    );
    assert!(started(&mut store, "third"));
    let again = avatars::get(&store, "a1").unwrap().unwrap();
    assert_eq!(
        (again.build_state, again.failure_kind),
        (AvatarBuildState::Building, None)
    );
    store
        .connection()
        .execute("UPDATE avatars SET build_state='ready' WHERE id='a1'", [])
        .unwrap();
    assert!(!started(&mut store, "fourth"));
}

#[test]
fn a_binding_is_replaced_and_keeps_the_clients_with_voice_on() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "One"));
    add(&mut store, "a2", &avatar("a2", "Two"));
    open_workspace(&mut store, "w1");
    commit(&mut store, "bind", |tx| {
        avatars::bind_in(tx, &binding("w1", "a1"))
    });
    commit(&mut store, "on-local", |tx| {
        avatars::set_voice_in(tx, "w1", "local", true)
    });
    commit(&mut store, "on-phone", |tx| {
        avatars::set_voice_in(tx, "w1", "phone", true)
    });
    commit(&mut store, "on-again", |tx| {
        avatars::set_voice_in(tx, "w1", "phone", true)
    });
    let mut other = binding("w1", "a2");
    other.persona = String::new();
    other.hearing = None;
    commit(&mut store, "rebind", |tx| avatars::bind_in(tx, &other));
    let kept = avatars::binding(&store, "w1").unwrap().unwrap();
    assert_eq!(kept.avatar_id, "a2");
    assert_eq!((kept.persona.as_str(), kept.hearing), ("", None));
    assert_eq!(kept.voice_on, ["local", "phone"]);
    commit(&mut store, "off", |tx| {
        avatars::set_voice_in(tx, "w1", "local", false)
    });
    assert_eq!(
        avatars::binding(&store, "w1").unwrap().unwrap().voice_on,
        ["phone"]
    );
}

#[test]
fn voice_is_only_turned_on_for_a_workspace_that_has_an_avatar() {
    let profile = Profile::new();
    let mut store = profile.store();
    open_workspace(&mut store, "w1");
    commit(&mut store, "on", |tx| {
        avatars::set_voice_in(tx, "w1", "local", true)
    });
    assert_eq!(avatars::binding(&store, "w1").unwrap(), None);
}

#[test]
fn only_open_workspaces_hold_an_avatar_and_deleting_it_takes_the_closed_ones_bindings() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "One"));
    open_workspace(&mut store, "w1");
    open_workspace(&mut store, "w2");
    commit(&mut store, "b1", |tx| {
        avatars::bind_in(tx, &binding("w1", "a1"))
    });
    commit(&mut store, "b2", |tx| {
        avatars::bind_in(tx, &binding("w2", "a1"))
    });
    commit(&mut store, "v2", |tx| {
        avatars::set_voice_in(tx, "w2", "local", true)
    });
    assert_eq!(
        avatars::open_workspaces_of(&store, "a1").unwrap(),
        ["w1", "w2"]
    );
    commit(&mut store, "close", |tx| tx.close_workspace("w2"));
    assert_eq!(avatars::open_workspaces_of(&store, "a1").unwrap(), ["w1"]);
    let listed: Vec<_> = avatars::bindings(&store)
        .unwrap()
        .into_iter()
        .map(|binding| binding.workspace_id)
        .collect();
    assert_eq!(listed, ["w1"]);
    commit(&mut store, "unbound", |tx| tx.close_workspace("w1"));
    commit(&mut store, "delete", |tx| {
        avatars::delete_in(tx, "a1").map(drop)
    });
    assert_eq!(avatars::get(&store, "a1").unwrap(), None);
    assert_eq!(avatars::binding(&store, "w1").unwrap(), None);
    let leftover: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM workspace_avatar_voice", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(leftover, 0);
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
fn a_stored_value_nobody_defined_is_an_error_and_not_a_guess() {
    let profile = Profile::new();
    let mut store = profile.store();
    add(&mut store, "a1", &avatar("a1", "Sabio"));
    store
        .connection()
        .execute("UPDATE avatars SET kind='hologram' WHERE id='a1'", [])
        .unwrap();
    assert!(avatars::get(&store, "a1").is_err());
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
            "DROP TABLE workspace_avatar_voice; DROP TABLE workspace_avatars; DROP TABLE avatars; PRAGMA user_version = 37;",
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
    commit(&mut store, "bind", |tx| {
        avatars::bind_in(tx, &binding("w1", "a1"))
    });
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
