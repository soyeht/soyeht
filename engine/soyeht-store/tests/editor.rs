#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::{
    Actor, EditorLanguage, FileAuthor, FileVersion, Pane, PaneMode, PaneState, Role, StateSource,
    Workspace,
};
use soyeht_store::{
    database::Store,
    domain::PaneRecord,
    editor::{EditorFile, StoredFileVersion},
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-editor-store-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
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

fn author(name: &str) -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role: Role::HumanLocal,
            client: "mac".into(),
            device_id: None,
            pane_id: None,
        },
        name: name.into(),
    }
}

fn editor_file() -> EditorFile {
    EditorFile {
        file_id: "file-1".into(),
        pane_id: "pane-1".into(),
        workspace_id: "workspace-1".into(),
        relative_path: "src/main.rs".into(),
        language: EditorLanguage::Rust,
        revision: 2,
        disk_revision: 1,
        disk_text: "fn main() {}\n".into(),
        draft_text: Some("fn main() { work(); }\n".into()),
        draft_author: Some(author("Caio")),
        conflict_text: None,
        watching: true,
        updated_ms: 100,
    }
}

fn version(revision: u64, content: String) -> StoredFileVersion {
    StoredFileVersion {
        version: FileVersion {
            version_id: format!("version-{revision}"),
            file_id: "file-1".into(),
            revision: revision.to_string(),
            created_at: format!("2026-09-12T18:{:02}:00Z", revision % 60),
            author: author("Caio"),
            byte_count: content.len().to_string(),
        },
        content,
    }
}

fn seed(store: &mut Store) {
    let workspace = Workspace {
        workspace_id: "workspace-1".into(),
        name: "Editor".into(),
        path: "/tmp/editor".into(),
        layout: json!({}),
    };
    let pane = PaneRecord {
        pane: Pane {
            pane_id: "pane-1".into(),
            workspace_id: "workspace-1".into(),
            name: "main.rs".into(),
            cwd: "/tmp/editor".into(),
            mode: PaneMode::Editor,
            state: PaneState::Idle,
            source: StateSource::Protocol,
            confidence: 1.0,
            phrase: "editing main.rs".into(),
            generation: 1,
            cols: 80,
            rows: 24,
            record: false,
            pid: None,
            stream_id: None,
            agent: None,
        },
        closed: false,
        instance_key: "editor-pane-1".into(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    };
    store
        .mutate("test", "seed", "seed", |change| {
            change.insert_workspace(&workspace)?;
            change.insert_pane(&pane)?;
            change.save_editor_file(&editor_file())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn save_version(store: &mut Store, item: &StoredFileVersion) {
    let key = item.version.version_id.clone();
    store
        .mutate("test", &key, &key, |change| {
            change.record_file_version(item)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

#[test]
fn an_unsaved_draft_and_conflict_survive_reopen_without_losing_the_disk_base() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let mut conflicted = editor_file();
    conflicted.revision = 3;
    conflicted.conflict_text = Some("fn changed_elsewhere() {}\n".into());
    conflicted.updated_ms = 200;
    store
        .mutate("test", "conflict", "conflict", |change| {
            change.save_editor_file(&conflicted)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);

    let store = profile.store();
    assert_eq!(
        store.editor_file("file-1").unwrap(),
        Some(conflicted.clone())
    );
    assert_eq!(
        store.editor_file_for_pane("pane-1").unwrap(),
        Some(conflicted)
    );
    assert_eq!(store.editor_files().unwrap().len(), 1);
}

#[test]
fn history_is_newest_first_paged_exclusively_and_restores_exact_content() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    for revision in 1..=3 {
        save_version(
            &mut store,
            &version(revision, format!("revision {revision}\n")),
        );
    }

    let first = store.file_history("file-1", None, 2).unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.revision.as_str())
            .collect::<Vec<_>>(),
        vec!["3", "2"]
    );
    let second = store
        .file_history("file-1", first.next_cursor.as_deref(), 2)
        .unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].revision, "1");
    assert_eq!(second.next_cursor, None);
    assert_eq!(
        store
            .file_version("file-1", "version-2")
            .unwrap()
            .unwrap()
            .content,
        "revision 2\n"
    );
}

#[test]
fn history_retention_enforces_both_the_count_and_byte_caps_in_the_transaction() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    for revision in 1..=202 {
        save_version(&mut store, &version(revision, format!("{revision}\n")));
    }
    let count_limited = store.file_history("file-1", None, 200).unwrap();
    assert_eq!(count_limited.items.len(), 200);
    assert_eq!(count_limited.items.last().unwrap().revision, "3");

    let large = "x".repeat(27 * 1024 * 1024);
    save_version(&mut store, &version(203, large.clone()));
    save_version(&mut store, &version(204, large));
    let byte_limited = store.file_history("file-1", None, 200).unwrap();
    assert_eq!(byte_limited.items.len(), 1);
    assert_eq!(byte_limited.items[0].revision, "204");
}

#[test]
fn migration_from_populated_v11_preserves_existing_rows_and_adds_editor_storage() {
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
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v11-db'),('engine_generation','0');
               INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"Editor","path":"/tmp/editor","layout":{}}',0);
               INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES ('pane-1','workspace-1','{"paneId":"pane-1","workspaceId":"workspace-1","name":"main.rs","cwd":"/tmp/editor","mode":"editor","state":"idle","source":"protocol","confidence":1.0,"phrase":"","generation":1,"cols":80,"rows":24,"record":false}',0,'editor-pane-1','0');
               INSERT INTO usage_readings(provider,observed_at_unix_sec,payload) VALUES ('codex',1,'{"provider":"codex","state":"unavailable","source":"codex_app_server","fetchedAt":"2026-09-12T18:00:00Z"}');"#,
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert!(store.usage_readings().unwrap().len() == 1);
    store
        .mutate("test", "editor", "editor", |change| {
            change.save_editor_file(&editor_file())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.editor_file("file-1").unwrap().is_some());
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}
