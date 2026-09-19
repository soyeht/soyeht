#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::{
    Actor, BeatCard, BeatCategory, EditorLanguage, FileAuthor, Role, WritingFont, WritingKind,
    WritingLineVariants, WritingSection, WritingVariant,
};
use soyeht_store::{
    database::{Error, Store},
    domain::PaneRecord,
    editor::EditorFile,
    writing::{WritingCreateIntent, WritingDocumentRecord, WritingSceneAnnotation, WritingSidecar},
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-writing-store-{}", Uuid::new_v4()));
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

fn author(role: Role, pane_id: Option<&str>) -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role,
            client: "test".into(),
            device_id: None,
            pane_id: pane_id.map(str::to_owned),
        },
        name: "Writer".into(),
    }
}

fn seed_file(store: &mut Store) {
    let workspace = soyeht_protocol::Workspace {
        workspace_id: "workspace-1".into(),
        name: "Writing".into(),
        path: "/tmp/writing".into(),
        layout: json!({}),
    };
    let pane: soyeht_protocol::Pane = serde_json::from_value(json!({
        "paneId": "pane-1", "workspaceId": "workspace-1", "name": "Draft",
        "cwd": "/tmp/writing", "mode": "writing", "state": "idle",
        "source": "protocol", "confidence": 1.0, "phrase": "",
        "generation": 1, "cols": 80, "rows": 24, "record": false,
        "writingKind": "screenplay", "writingSection": "screenplays"
    }))
    .unwrap();
    let file = EditorFile {
        file_id: "file-1".into(),
        pane_id: Some("pane-1".into()),
        workspace_id: "workspace-1".into(),
        relative_path: "Writing/Screenplays/draft.fountain".into(),
        language: EditorLanguage::PlainText,
        revision: 1,
        disk_revision: 1,
        disk_text: "INT. ROOM - DAY\n".into(),
        draft_text: None,
        draft_author: None,
        conflict_text: None,
        watching: true,
        updated_ms: 1,
    };
    store
        .mutate("seed", "writing", "seed", |change| {
            change.insert_workspace(&workspace)?;
            change.insert_pane(&PaneRecord {
                pane,
                closed: false,
                instance_key: "writing-pane-1".into(),
                instance_id: None,
                supervisor_id: None,
                sequence_base: 0,
            })?;
            change.save_editor_file(&file)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn document() -> WritingDocumentRecord {
    WritingDocumentRecord {
        file_id: "file-1".into(),
        kind: WritingKind::Screenplay,
        section: WritingSection::Screenplays,
        sidecar_revision: 0,
        font: WritingFont::ReadingSerif,
        focus_mode: false,
        full_width: false,
        sidecar: WritingSidecar::default(),
        updated_ms: 2,
    }
}

fn sidecar() -> WritingSidecar {
    WritingSidecar {
        line_variants: vec![WritingLineVariants {
            line_id: "line-1".into(),
            line: 2,
            alternates: vec![
                WritingVariant {
                    variant_id: "variant-1".into(),
                    text: "What time is it?".into(),
                    author: author(Role::HumanLocal, None),
                },
                WritingVariant {
                    variant_id: "variant-2".into(),
                    text: "Do you know the time?".into(),
                    author: author(Role::Agent, Some("agent-1")),
                },
            ],
            selected_index: 1,
        }],
        beat_cards: vec![BeatCard {
            beat_id: "beat-1".into(),
            title: "The clock".into(),
            body: "Time becomes urgent.".into(),
            category: BeatCategory::Story,
            order: 0,
            scene_id: Some("scene-1".into()),
        }],
        scene_annotations: vec![WritingSceneAnnotation {
            scene_id: "scene-1".into(),
            reference_image: Some("references/clock.png".into()),
            caption: Some("A stopped clock".into()),
            prompt_idea: Some("Close view of the hands".into()),
        }],
    }
}

fn create_intent() -> WritingCreateIntent {
    WritingCreateIntent {
        scope: "client:local".into(),
        idempotency_key: "create-draft".into(),
        fingerprint: "request-fingerprint".into(),
        operation_id: "operation-1".into(),
        workspace_id: "workspace-1".into(),
        relative_path: "Writing/Screenplays/draft-two.fountain".into(),
        file_id: "file-2".into(),
        pane_id: "pane-2".into(),
        kind: WritingKind::Screenplay,
        section: WritingSection::Screenplays,
        created_ms: 4,
    }
}

#[test]
fn document_create_intent_survives_reopen_and_is_removed_with_the_final_receipt() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_file(&mut store);
    let intent = create_intent();
    store.record_writing_create_intent(&intent).unwrap();
    drop(store);

    let mut store = profile.store();
    assert_eq!(
        store
            .writing_create_intent(&intent.scope, &intent.idempotency_key)
            .unwrap(),
        Some(intent.clone())
    );
    store
        .mutate(
            "client:local",
            "create-draft",
            "request-fingerprint",
            |change| {
                change.remove_writing_create_intent(&intent.scope, &intent.idempotency_key)?;
                Ok((json!({"created":true}), vec![]))
            },
        )
        .unwrap();
    assert_eq!(
        store
            .writing_create_intent(&intent.scope, &intent.idempotency_key)
            .unwrap(),
        None
    );

    let mut invalid = intent;
    invalid.kind = WritingKind::Text;
    assert!(matches!(
        store.record_writing_create_intent(&invalid),
        Err(Error::InvalidWritingCreateIntent)
    ));
}

#[test]
fn writing_document_preferences_replace_without_bypassing_the_sidecar_journal() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_file(&mut store);
    let mut initial = document();
    initial.full_width = true;
    store
        .mutate("test", "initial", "initial", |change| {
            change.save_writing_document(&initial)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let mut preferred = initial.clone();
    preferred.font = WritingFont::Sans;
    preferred.focus_mode = true;
    preferred.updated_ms = 3;
    store
        .mutate("test", "preference", "preference", |change| {
            change.save_writing_document(&preferred)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(store.writing_document("file-1").unwrap(), Some(preferred));

    let mut bypass = initial;
    bypass.sidecar_revision = 1;
    bypass.sidecar = sidecar();
    let error = store
        .mutate("test", "bypass", "bypass", |change| {
            change.save_writing_document(&bypass)?;
            Ok((json!({}), vec![]))
        })
        .err()
        .expect("sidecar bypass must fail");
    assert!(matches!(error, Error::InvalidWritingDocument));
}

#[test]
fn one_transaction_advances_the_sidecar_and_preserves_the_writer_selection_after_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_file(&mut store);
    let mut initial = document();
    initial.full_width = true;
    store
        .mutate("test", "initial", "initial", |change| {
            change.save_writing_document(&initial)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let mut updated = initial;
    updated.sidecar_revision = 1;
    updated.sidecar = sidecar();
    updated.updated_ms = 3;
    let writer = author(Role::HumanLocal, None);
    store
        .mutate("test", "sidecar", "sidecar", |change| {
            change.advance_writing_sidecar(0, &updated, &writer, 3)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);

    let reopened = profile.store();
    let stored = reopened.writing_document("file-1").unwrap().unwrap();
    assert!(stored.full_width);
    assert_eq!(stored.sidecar.line_variants[0].selected_index, 1);
    let versions = reopened.writing_sidecar_versions("file-1").unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].sidecar, sidecar());
}

#[test]
fn invalid_kind_section_and_sidecar_bounds_fail_before_sql() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_file(&mut store);
    let mut invalid = document();
    invalid.section = WritingSection::Texts;
    let error = store
        .mutate("test", "kind", "kind", |change| {
            change.save_writing_document(&invalid)?;
            Ok((json!({}), vec![]))
        })
        .err()
        .expect("mismatched kind and section must fail");
    assert!(matches!(error, Error::InvalidWritingDocument));
    assert!(store.writing_document("file-1").unwrap().is_none());

    let valid = document();
    store
        .mutate("test", "valid", "valid", |change| {
            change.save_writing_document(&valid)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let mut oversized = valid;
    oversized.sidecar_revision = 1;
    oversized.sidecar.line_variants = vec![WritingLineVariants {
        line_id: "line-1".into(),
        line: 0,
        alternates: vec![WritingVariant {
            variant_id: "variant-1".into(),
            text: "x".repeat(soyeht_protocol::limits::WRITING_VARIANT_BYTES + 1),
            author: author(Role::Agent, Some("agent-1")),
        }],
        selected_index: 0,
    }];
    let error = store
        .mutate("test", "oversized", "oversized", |change| {
            change.advance_writing_sidecar(
                0,
                &oversized,
                &author(Role::Agent, Some("agent-1")),
                3,
            )?;
            Ok((json!({}), vec![]))
        })
        .err()
        .expect("oversized sidecar must fail");
    assert!(matches!(error, Error::InvalidWritingSidecar));
    assert_eq!(
        store
            .writing_document("file-1")
            .unwrap()
            .unwrap()
            .sidecar_revision,
        0
    );
}

#[test]
fn migration_from_populated_v20_preserves_writing_data_and_adds_create_intents() {
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
        include_str!("../migrations/015-editor-agent-reviews.sql"),
        include_str!("../migrations/016-web-session-grants.sql"),
        include_str!("../migrations/017-web-permissions.sql"),
        include_str!("../migrations/018-web-permission-operations.sql"),
        include_str!("../migrations/019-agent-prompt-attachments.sql"),
        include_str!("../migrations/020-writing-documents.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v19-db'),('engine_generation','0');
               INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"Writing","path":"/tmp/writing","layout":{}}',0);
               INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES ('pane-1','workspace-1','{"paneId":"pane-1","workspaceId":"workspace-1","name":"Draft","cwd":"/tmp/writing","mode":"writing","state":"idle","source":"protocol","confidence":1.0,"phrase":"","generation":1,"cols":80,"rows":24,"record":false,"writingKind":"screenplay","writingSection":"screenplays"}',0,'writing-pane-1','0');
               INSERT INTO editor_files(file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms) VALUES ('file-1','pane-1','workspace-1','Writing/Screenplays/draft.fountain','plain_text',1,1,'INT. ROOM - DAY\n',NULL,NULL,NULL,1,1);
               INSERT INTO writing_documents(file_id,kind,section,sidecar_revision,font,focus_mode,sidecar_payload,updated_ms) VALUES ('file-1','screenplay','screenplays',0,'reading_serif',0,'{"lineVariants":[],"beatCards":[],"sceneAnnotations":[]}',2);"#,
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(store.editor_file("file-1").unwrap().unwrap().revision, 1);
    let document = document();
    store
        .mutate("test", "document", "document", |change| {
            change.save_writing_document(&document)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(store.writing_document("file-1").unwrap(), Some(document));
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}
