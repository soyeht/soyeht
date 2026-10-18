#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use soyeht_protocol::{
    Actor, EditorLanguage, FileAuthor, FileSearchMatch, FileVersion, Pane, PaneMode, Role,
    TextPosition, TextRange, Workspace,
};
use soyeht_store::{
    database::Store,
    domain::PaneRecord,
    editor::{
        EditorAgentReview, EditorFile, ProjectReplacement, ProjectReplacementItem,
        StoredFileVersion,
    },
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

fn agent_author(pane_id: &str, name: &str) -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role: Role::Agent,
            client: "engine".into(),
            device_id: None,
            pane_id: Some(pane_id.into()),
        },
        name: name.into(),
    }
}

fn agent_review() -> EditorAgentReview {
    EditorAgentReview {
        file_id: "file-1".into(),
        revision: 2,
        pane_id: "pane-1".into(),
        author: agent_author("pane-1", "Builder"),
        baseline_text: "fn main() {}\n".into(),
        updated_ms: 100,
    }
}

fn editor_file() -> EditorFile {
    EditorFile {
        file_id: "file-1".into(),
        pane_id: Some("pane-1".into()),
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

fn replacement(files: &[EditorFile]) -> ProjectReplacement {
    let items = files
        .iter()
        .enumerate()
        .map(|(ordinal, file)| ProjectReplacementItem {
            ordinal: ordinal as u32,
            file_id: file.file_id.clone(),
            relative_path: file.relative_path.clone(),
            before_hash: "a".repeat(64),
            after_hash: "b".repeat(64),
            replaced: 1,
            applied: false,
        })
        .collect::<Vec<_>>();
    ProjectReplacement {
        operation_id: "replace-1".into(),
        scope: "client-1".into(),
        idempotency_key: "request-1".into(),
        fingerprint: "fingerprint-1".into(),
        workspace_id: "workspace-1".into(),
        query: "before".into(),
        replacement: "after".into(),
        case_sensitive: true,
        author: author("Caio"),
        matches: files
            .iter()
            .map(|file| FileSearchMatch {
                workspace_id: "workspace-1".into(),
                path: file.relative_path.clone(),
                range: TextRange {
                    start: TextPosition { line: 0, column: 0 },
                    end: TextPosition { line: 0, column: 6 },
                },
                preview: "before".into(),
            })
            .collect(),
        total_replaced: items.len() as u32,
        created_ms: 200,
        items,
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
            name: "main.rs".into(),
            phrase: "editing main.rs".into(),
            ..support::pane("pane-1", "workspace-1", PaneMode::Editor, "/tmp/editor")
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
fn one_agent_review_per_file_replaces_atomically_survives_reopen_and_clears() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let first = agent_review();
    store
        .mutate("test", "review-1", "review-1", |change| {
            change.save_editor_agent_review(&first)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);

    let mut store = profile.store();
    assert_eq!(store.editor_agent_review("file-1").unwrap(), Some(first));
    let mut updated = agent_review();
    updated.revision = 3;
    updated.baseline_text = "fn main() { accepted(); }\n".into();
    updated.updated_ms = 200;
    store
        .mutate("test", "review-2", "review-2", |change| {
            change.save_editor_agent_review(&updated)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(store.editor_agent_review("file-1").unwrap(), Some(updated));
    store
        .mutate("test", "review-clear", "review-clear", |change| {
            assert!(change.clear_editor_agent_review("file-1")?);
            assert!(!change.clear_editor_agent_review("file-1")?);
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.editor_agent_review("file-1").unwrap().is_none());
}

#[test]
fn invalid_agent_review_is_rejected_before_sql() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let mut invalid = agent_review();
    invalid.author.actor.pane_id = Some("another-pane".into());
    let result = store.mutate("test", "invalid-review", "invalid-review", |change| {
        change.save_editor_agent_review(&invalid)?;
        Ok((json!({}), vec![]))
    });
    assert!(matches!(
        result,
        Err(soyeht_store::Error::InvalidEditorAgentReview)
    ));
    assert!(store.editor_agent_review("file-1").unwrap().is_none());
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
fn replacement_journal_keeps_detached_history_and_closes_only_after_every_item() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let mut open = editor_file();
    open.draft_text = None;
    open.draft_author = None;
    open.disk_text = "before one\n".into();
    let detached = EditorFile {
        file_id: "file-2".into(),
        pane_id: None,
        workspace_id: "workspace-1".into(),
        relative_path: "src/other.rs".into(),
        language: EditorLanguage::Rust,
        revision: 1,
        disk_revision: 1,
        disk_text: "before two\n".into(),
        draft_text: None,
        draft_author: None,
        conflict_text: None,
        watching: false,
        updated_ms: 200,
    };
    store
        .mutate("test", "clean", "clean", |change| {
            change.save_editor_file(&open)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let journal = replacement(&[open.clone(), detached.clone()]);
    assert_eq!(
        store
            .begin_file_replacement(&journal, &[open.clone(), detached.clone()])
            .unwrap(),
        journal
    );
    let mut busy = journal.clone();
    busy.operation_id = "replace-2".into();
    busy.idempotency_key = "request-2".into();
    busy.fingerprint = "fingerprint-2".into();
    assert!(matches!(
        store.begin_file_replacement(&busy, &[open.clone(), detached.clone()]),
        Err(soyeht_store::Error::FileReplacementBusy)
    ));
    assert_eq!(
        store
            .begin_file_replacement(&journal, &[open.clone(), detached.clone()])
            .unwrap(),
        journal
    );
    let mut conflicting = journal.clone();
    conflicting.fingerprint = "different".into();
    assert!(matches!(
        store.begin_file_replacement(&conflicting, &[open.clone(), detached.clone()]),
        Err(soyeht_store::Error::IdempotencyConflict)
    ));
    drop(store);

    finish_replacement_after_reopen(&profile, journal, open, detached);
}

fn finish_replacement_after_reopen(
    profile: &Profile,
    journal: ProjectReplacement,
    open: EditorFile,
    detached: EditorFile,
) {
    let mut store = profile.store();
    assert_eq!(
        store.active_file_replacements().unwrap(),
        vec![journal.clone()]
    );
    assert_eq!(store.editor_file("file-2").unwrap(), Some(detached.clone()));
    let mut applied_open = open;
    applied_open.revision = 3;
    applied_open.disk_revision = 2;
    applied_open.disk_text = "after one\n".into();
    applied_open.updated_ms = 300;
    let open_version = StoredFileVersion {
        version: FileVersion {
            version_id: "replace-version-1".into(),
            file_id: "file-1".into(),
            revision: "3".into(),
            created_at: "2026-09-12T21:00:00Z".into(),
            author: author("Caio"),
            byte_count: "10".into(),
        },
        content: "after one\n".into(),
    };
    store
        .mutate("replace", "first", "first", |change| {
            assert!(change.mark_file_replacement_applied(
                "replace-1",
                0,
                &applied_open,
                &open_version,
            )?);
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(matches!(
        store.mutate("replace", "early", "early", |change| {
            change.complete_file_replacement("replace-1")?;
            Ok((json!({}), vec![]))
        }),
        Err(soyeht_store::Error::FileReplacementIncomplete)
    ));
    let mut applied_detached = detached;
    applied_detached.revision = 2;
    applied_detached.disk_revision = 2;
    applied_detached.disk_text = "after two\n".into();
    applied_detached.updated_ms = 300;
    let detached_version = StoredFileVersion {
        version: FileVersion {
            version_id: "replace-version-2".into(),
            file_id: "file-2".into(),
            revision: "2".into(),
            created_at: "2026-09-12T21:00:00Z".into(),
            author: author("Caio"),
            byte_count: "10".into(),
        },
        content: "after two\n".into(),
    };
    store
        .mutate("replace", "second", "second", |change| {
            assert!(change.mark_file_replacement_applied(
                "replace-1",
                1,
                &applied_detached,
                &detached_version,
            )?);
            change.complete_file_replacement("replace-1")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.active_file_replacements().unwrap().is_empty());
    assert_eq!(
        store.file_history("file-2", None, 10).unwrap().items[0].revision,
        "2"
    );
    assert_eq!(store.editor_files().unwrap(), vec![applied_open]);
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

#[test]
fn migration_from_populated_v12_preserves_open_files_and_history() {
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
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v12-db'),('engine_generation','0');
               INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"Editor","path":"/tmp/editor","layout":{}}',0);
               INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES ('pane-1','workspace-1','{"paneId":"pane-1","workspaceId":"workspace-1","name":"main.rs","cwd":"/tmp/editor","mode":"editor","state":"idle","source":"protocol","confidence":1.0,"phrase":"","generation":1,"cols":80,"rows":24,"record":false}',0,'editor-pane-1','0');"#,
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO editor_files VALUES ('file-1','pane-1','workspace-1','src/main.rs','rust',2,2,?1,NULL,NULL,NULL,1,200)",
            ["saved\n"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO editor_file_versions(version_id,file_id,revision,created_at,author,content,byte_count) VALUES ('version-2','file-1',2,'2026-09-12T21:00:00Z',?1,?2,6)",
            [r#"{"actor":{"role":"human_local","client":"mac"},"name":"Caio"}"#, "saved\n"],
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    let file = store.editor_file("file-1").unwrap().unwrap();
    assert_eq!(file.pane_id.as_deref(), Some("pane-1"));
    assert_eq!(file.disk_text, "saved\n");
    assert_eq!(
        store
            .file_version("file-1", "version-2")
            .unwrap()
            .unwrap()
            .content,
        "saved\n"
    );
    save_version(&mut store, &version(3, "next\n".into()));
    assert_eq!(
        store.file_history("file-1", None, 10).unwrap().items.len(),
        2
    );
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}

#[test]
fn migration_from_populated_v14_preserves_files_and_adds_agent_reviews() {
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
        include_str!("../migrations/014-language-servers.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v14-db'),('engine_generation','0');
               INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"Editor","path":"/tmp/editor","layout":{}}',0);
               INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES ('pane-1','workspace-1','{"paneId":"pane-1","workspaceId":"workspace-1","name":"main.rs","cwd":"/tmp/editor","mode":"editor","state":"idle","source":"protocol","confidence":1.0,"phrase":"","generation":1,"cols":80,"rows":24,"record":false}',0,'editor-pane-1','0');"#,
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO editor_files(file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms) VALUES ('file-1','pane-1','workspace-1','src/main.rs','rust',2,2,?1,NULL,NULL,NULL,1,200)",
            ["saved\n"],
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(
        store.editor_file("file-1").unwrap().unwrap().disk_text,
        "saved\n"
    );
    let review = agent_review();
    store
        .mutate("test", "review", "review", |change| {
            change.save_editor_agent_review(&review)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(store.editor_agent_review("file-1").unwrap(), Some(review));
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}
