#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::EditorLanguage;
use soyeht_store::{
    database::{SCHEMA_VERSION, Store},
    languages::LanguageServerCheckpoint,
};
use std::fs;
use uuid::Uuid;

struct Profile(std::path::PathBuf);

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("soyeht-languages-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn store(&self) -> Store {
        Store::open(&self.0).unwrap()
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn checkpoint(generation: u32) -> LanguageServerCheckpoint {
    LanguageServerCheckpoint {
        workspace_id: "workspace-1".into(),
        language: EditorLanguage::Rust,
        payload: json!({"generation":generation,"instanceKey":"lsp-rust"}),
    }
}

fn workspace(store: &mut Store) {
    store
        .mutate("test", "workspace", "workspace", |change| {
            change.insert_workspace(&soyeht_protocol::Workspace {
                workspace_id: "workspace-1".into(),
                name: "Workspace".into(),
                path: "/tmp/workspace".into(),
                layout: json!({}),
            })?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
}

#[test]
fn one_checkpoint_per_workspace_language_replaces_atomically_and_survives_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    workspace(&mut store);
    for generation in [1, 2] {
        store
            .mutate(
                "test",
                &format!("checkpoint-{generation}"),
                "checkpoint",
                |change| {
                    change.save_language_server_checkpoint(&checkpoint(generation))?;
                    Ok((json!({}), Vec::new()))
                },
            )
            .unwrap();
    }
    drop(store);
    let store = profile.store();
    assert_eq!(
        store.language_server_checkpoints().unwrap(),
        vec![checkpoint(2)]
    );
}

#[test]
fn invalid_language_and_payload_are_rejected_before_sql() {
    let profile = Profile::new();
    let mut store = profile.store();
    workspace(&mut store);
    for invalid in [
        LanguageServerCheckpoint {
            language: EditorLanguage::Markdown,
            ..checkpoint(1)
        },
        LanguageServerCheckpoint {
            payload: json!([]),
            ..checkpoint(1)
        },
    ] {
        let result = store.mutate(
            "test",
            &Uuid::new_v4().to_string(),
            "checkpoint",
            |change| {
                change.save_language_server_checkpoint(&invalid)?;
                Ok((json!({}), Vec::new()))
            },
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("invalid checkpoint was stored"),
        };
        assert!(matches!(
            error,
            soyeht_store::Error::InvalidLanguageServerCheckpoint
        ));
    }
    assert!(store.language_server_checkpoints().unwrap().is_empty());
}

#[test]
fn migration_from_populated_v13_preserves_existing_rows_and_adds_checkpoints() {
    let profile = Profile::new();
    let connection = rusqlite::Connection::open(profile.0.join("soyeht.db")).unwrap();
    for migration in [
        include_str!("../migrations/001-initial.sql"),
        include_str!("../migrations/002-client-state.sql"),
        include_str!("../migrations/003-english-values.sql"),
        include_str!("../migrations/004-agent-sessions.sql"),
        include_str!("../migrations/005-native-replay.sql"),
        include_str!("../migrations/006-agent-auth-snapshot.sql"),
        include_str!("../migrations/007-workspace-tasks.sql"),
        include_str!("../migrations/008-task-reviews.sql"),
        include_str!("../migrations/009-task-block-retries.sql"),
        include_str!("../migrations/010-agent-pane-role.sql"),
        include_str!("../migrations/011-usage-readings.sql"),
        include_str!("../migrations/012-editor-files.sql"),
        include_str!("../migrations/013-editor-project-replacements.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v13-db'),('engine_generation','0');
               INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"Editor","path":"/tmp/editor","layout":{}}',0);
               INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES ('pane-1','workspace-1','{"paneId":"pane-1","workspaceId":"workspace-1","name":"main.rs","cwd":"/tmp/editor","mode":"editor","state":"idle","source":"protocol","confidence":1.0,"phrase":"","generation":1,"cols":80,"rows":24,"record":false}',0,'editor-pane-1','0');"#,
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert!(store.editor_file("file-1").unwrap().is_none());
    store
        .mutate("test", "checkpoint", "checkpoint", |change| {
            change.save_language_server_checkpoint(&checkpoint(1))?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
    assert_eq!(
        store.language_server_checkpoints().unwrap(),
        vec![checkpoint(1)]
    );
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
}
