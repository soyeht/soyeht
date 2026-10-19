#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::Workspace;
use soyeht_store::{
    database::{Error, Store},
    segments::{Log, Record},
};
use std::{fs, io::Write, path::PathBuf};
use uuid::Uuid;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("soyeht-next-store-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn workspace(id: &str, name: String) -> Workspace {
    Workspace {
        workspace_id: id.into(),
        name,
        path: "/tmp".into(),
        layout: json!({}),
    }
}

#[test]
fn state_event_and_idempotent_result_commit_together_and_survive_reopen() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0).unwrap();
    let id = store.db_id.clone();
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "synchronous", |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(store.next_engine_generation().unwrap(), 1);
    let first = store
        .mutate("local:test", "key-1", "request-1", |change| {
            change.insert_workspace(&workspace("w1", "Workspace".into()))?;
            Ok((
                json!({"workspaceId":"w1"}),
                vec![(
                    "workspace.created".into(),
                    json!({"workspace":{"workspaceId":"w1"}}),
                )],
            ))
        })
        .unwrap();
    assert_eq!(first.events.len(), 1);
    let repeated = store
        .mutate("local:test", "key-1", "request-1", |_| {
            panic!("repeated operation executed")
        })
        .unwrap();
    assert!(repeated.repeated);
    assert_eq!(first.result, repeated.result);
    assert!(matches!(
        store.mutate("local:test", "key-1", "different", |_| unreachable!()),
        Err(Error::IdempotencyConflict)
    ));
    drop(store);
    let mut store = Store::open(&directory.0).unwrap();
    assert_eq!(store.db_id, id);
    assert_eq!(store.next_engine_generation().unwrap(), 2);
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.cursor, 1);
    assert_eq!(snapshot.sections[0].1.as_array().unwrap().len(), 1);
    let (cursor, events) = store.replay(0).unwrap();
    assert_eq!(cursor, 1);
    assert_eq!(events[0].params["eventId"], "1");
    assert!(store.replay(1).unwrap().1.is_empty());
}

#[test]
fn failed_mutation_leaves_no_state_event_or_idempotency_receipt() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0).unwrap();
    let failed = store.mutate("local:test", "key-1", "request", |change| {
        change.insert_workspace(&workspace("w1", "Workspace".into()))?;
        Err(Error::TestFixture("injected failure"))
    });
    assert!(failed.is_err());
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.cursor, 0);
    assert!(snapshot.sections[0].1.as_array().unwrap().is_empty());
    let retry = store
        .mutate("local:test", "key-1", "request", |_| {
            Ok((json!("retry"), Vec::new()))
        })
        .unwrap();
    assert!(!retry.repeated);
}

#[test]
fn old_cursor_is_rejected_even_after_all_events_are_pruned() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0).unwrap();
    store
        .mutate("system", "first", "event", |_| {
            Ok((
                json!({}),
                vec![("engine.restarted".into(), json!({"generation":1}))],
            ))
        })
        .unwrap();
    store
        .connection()
        .execute("UPDATE events SET created_unix=0", [])
        .unwrap();
    store.prune().unwrap();
    assert!(matches!(store.replay(0), Err(Error::CursorTooOld)));
    assert_eq!(store.replay(1).unwrap().0, 1);
}

#[test]
fn future_database_version_is_refused() {
    let directory = Directory::new();
    let store = Store::open(&directory.0).unwrap();
    store
        .connection()
        .execute_batch("PRAGMA user_version=999")
        .unwrap();
    drop(store);
    assert!(matches!(
        Store::open(&directory.0),
        Err(Error::FutureVersion(999))
    ));
}

#[test]
fn migration_backs_up_an_existing_database_before_changing_version() {
    let directory = Directory::new();
    let old = rusqlite::Connection::open(directory.0.join("soyeht.db")).unwrap();
    old.execute_batch("PRAGMA user_version=0").unwrap();
    drop(old);
    let store = Store::open(&directory.0).unwrap();
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
    let backups: Vec<_> = fs::read_dir(directory.0.join("backups")).unwrap().collect();
    assert_eq!(backups.len(), 1);
    let backup = rusqlite::Connection::open(backups[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn version_23_migration_backfills_durable_workspace_and_pane_creation_sequences() {
    let directory = Directory::new();
    let old = rusqlite::Connection::open(directory.0.join("soyeht.db")).unwrap();
    old.execute_batch(
        "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO metadata(key,value) VALUES
           ('db_id','stable-id'),('engine_generation','0');
         CREATE TABLE workspaces (
           id TEXT PRIMARY KEY,
           payload TEXT NOT NULL,
           closed INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE panes (
           id TEXT PRIMARY KEY,
           workspace_id TEXT NOT NULL REFERENCES workspaces(id),
           payload TEXT NOT NULL,
           closed INTEGER NOT NULL DEFAULT 0,
           instance_key TEXT NOT NULL UNIQUE,
           instance_id TEXT,
           supervisor_id TEXT,
           sequence_base TEXT NOT NULL DEFAULT '0'
         );
         PRAGMA user_version=23;",
    )
    .unwrap();
    for workspace_id in [
        "z-last-alphabetically",
        "a-first-alphabetically",
        "m-middle",
    ] {
        let payload = serde_json::to_string(&workspace(workspace_id, workspace_id.into())).unwrap();
        old.execute(
            "INSERT INTO workspaces(id,payload) VALUES (?1,?2)",
            rusqlite::params![workspace_id, payload],
        )
        .unwrap();
        old.execute(
            "INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES (?1,?1,'{}',?1)",
            [workspace_id],
        )
        .unwrap();
    }
    drop(old);

    let store = Store::open(&directory.0).unwrap();
    assert_eq!(
        store
            .connection()
            .prepare("SELECT id,created_seq FROM workspaces ORDER BY created_seq")
            .unwrap()
            .query_map([], |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?
            )))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap(),
        [
            ("z-last-alphabetically".into(), 1),
            ("a-first-alphabetically".into(), 2),
            ("m-middle".into(), 3)
        ]
    );
    assert_eq!(
        store
            .connection()
            .prepare("SELECT id,created_seq FROM panes ORDER BY created_seq")
            .unwrap()
            .query_map([], |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?
            )))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap(),
        [
            ("z-last-alphabetically".into(), 1),
            ("a-first-alphabetically".into(), 2),
            ("m-middle".into(), 3)
        ]
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
fn full_database_does_not_confirm_a_partial_mutation() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0).unwrap();
    let pages = store
        .connection()
        .pragma_query_value(None, "page_count", |row| row.get::<_, u32>(0))
        .unwrap();
    store
        .connection()
        .pragma_update(None, "max_page_count", pages)
        .unwrap();
    let result = store.mutate("local:test", "large", "large-request", |change| {
        change.insert_workspace(&workspace("large", "x".repeat(2 * 1024 * 1024)))?;
        Ok((json!({}), vec![("workspace.created".into(), json!({}))]))
    });
    assert!(matches!(result, Err(Error::DiskFull)));
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.cursor, 0);
    assert!(snapshot.sections[0].1.as_array().unwrap().is_empty());
    let receipts: u32 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM idempotency", [], |row| row.get(0))
        .unwrap();
    assert_eq!(receipts, 0);
}

#[test]
fn version_one_migration_preserves_identity_and_records_the_actor() {
    let directory = Directory::new();
    let old = rusqlite::Connection::open(directory.0.join("soyeht.db")).unwrap();
    old.execute_batch(include_str!("../migrations/001-initial.sql"))
        .unwrap();
    old.execute(
        "INSERT INTO metadata(key,value) VALUES ('db_id','stable-id'),('engine_generation','7')",
        [],
    )
    .unwrap();
    drop(old);
    let mut store = Store::open(&directory.0).unwrap();
    assert_eq!(store.db_id, "stable-id");
    assert_eq!(store.next_engine_generation().unwrap(), 8);
    let actor = json!({"role":"human_local","client":"test"});
    let changed = store
        .mutate_as("local:test", "key", "request", Some(&actor), |change| {
            change.insert_workspace(&workspace("w1", "Workspace".into()))?;
            change.close_workspace("w1")?;
            Ok((
                json!({}),
                vec![("workspace.closed".into(), json!({"workspaceId":"w1"}))],
            ))
        })
        .unwrap();
    assert_eq!(changed.events[0].params["actor"], actor);
    let recorded: String = store
        .connection()
        .query_row("SELECT actor FROM idempotency WHERE key='key'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&recorded).unwrap(),
        actor
    );
    assert!(
        store.snapshot().unwrap().sections[0]
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_explicit_domain_actor_is_not_replaced_by_the_request_actor() {
    let directory = Directory::new();
    let mut store = Store::open(&directory.0).unwrap();
    let request_actor = json!({"role":"human_local","client":"test"});
    let changed = store
        .mutate_as(
            "local:test",
            "domain-actor",
            "domain-actor-request",
            Some(&request_actor),
            |_| {
                Ok((
                    json!({}),
                    vec![("task.moved".into(), json!({"actor":"you"}))],
                ))
            },
        )
        .unwrap();
    assert_eq!(changed.events[0].params["actor"], "you");
    let stored_actor: String = store
        .connection()
        .query_row(
            "SELECT actor FROM idempotency WHERE key='domain-actor'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored_actor).unwrap(),
        request_actor
    );
}

#[test]
fn output_gap_and_delta_survive_reopen_without_inventing_bytes() {
    let directory = Directory::new();
    let mut log = Log::open(&directory.0).unwrap();
    log.append(Record::Data {
        offset: 0,
        bytes: b"abcdef".to_vec(),
    })
    .unwrap();
    log.append(Record::Gap { from: 6, to: 100 }).unwrap();
    log.append(Record::Data {
        offset: 100,
        bytes: b"xyz".to_vec(),
    })
    .unwrap();
    drop(log);
    let log = Log::open(&directory.0).unwrap();
    assert_eq!(log.next_offset(), 103);
    assert_eq!(
        log.read(3, 103, 65536).unwrap(),
        vec![
            Record::Data {
                offset: 3,
                bytes: b"def".to_vec()
            },
            Record::Gap { from: 6, to: 100 },
            Record::Data {
                offset: 100,
                bytes: b"xyz".to_vec()
            }
        ]
    );
    assert_eq!(log.align_forward(4), 6);
    assert_eq!(log.align_forward(101), 103);
}

#[test]
fn incomplete_last_record_is_truncated_but_checksum_corruption_is_refused() {
    let directory = Directory::new();
    let mut log = Log::open(&directory.0).unwrap();
    log.append(Record::Data {
        offset: 0,
        bytes: b"stable".to_vec(),
    })
    .unwrap();
    drop(log);
    let path = directory.0.join("0000000000000000.pty");
    let good_length = fs::metadata(&path).unwrap().len();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[0, 0, 0])
        .unwrap();
    let log = Log::open(&directory.0).unwrap();
    assert!(log.recovered_truncated_tail);
    assert_eq!(log.next_offset(), 6);
    assert_eq!(fs::metadata(&path).unwrap().len(), good_length);
    drop(log);
    let partial_header = directory.0.join("0000000000000001.pty");
    fs::write(&partial_header, b"SOY").unwrap();
    let log = Log::open(&directory.0).unwrap();
    assert!(log.recovered_truncated_tail);
    assert_eq!(log.next_offset(), 6);
    assert!(!partial_header.exists());
    drop(log);
    let mut bytes = fs::read(&path).unwrap();
    bytes[21] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(Log::open(&directory.0).is_err());
}

#[test]
fn unsynced_records_are_readable_and_owe_one_flush_per_segment_until_it_is_taken() {
    let directory = Directory::new();
    let mut log = Log::open(&directory.0).unwrap();
    assert!(log.take_unsynced().is_none());
    // Enough output to fill the first segment and move on to the next.
    for n in 0..10 {
        log.append_unsynced(Record::Data {
            offset: n * 65536,
            bytes: vec![66; 65536],
        })
        .unwrap();
    }
    assert_eq!(log.next_offset(), 10 * 65536);
    assert_eq!(
        log.read(0, 3, 3).unwrap(),
        vec![Record::Data {
            offset: 0,
            bytes: vec![66; 3]
        }]
    );

    let owed = log.take_unsynced().unwrap();
    owed.flush().unwrap();
    assert!(log.take_unsynced().is_none(), "the flush is owed once");

    // A flush that failed is owed again, and a later synchronous append covers its segment.
    log.append_unsynced(Record::Gap {
        from: 10 * 65536,
        to: 10 * 65536 + 1,
    })
    .unwrap();
    let failed = log.take_unsynced().unwrap();
    log.restore_unsynced(&failed);
    assert!(log.take_unsynced().is_some());
    log.append_unsynced(Record::Gap {
        from: 10 * 65536 + 1,
        to: 10 * 65536 + 2,
    })
    .unwrap();
    log.append(Record::Gap {
        from: 10 * 65536 + 2,
        to: 10 * 65536 + 3,
    })
    .unwrap();
    assert!(log.take_unsynced().is_none());

    drop(log);
    let log = Log::open(&directory.0).unwrap();
    assert_eq!(log.next_offset(), 10 * 65536 + 3);
}

#[test]
fn segments_rotate_and_retention_removes_only_whole_old_segments() {
    let directory = Directory::new();
    let mut log = Log::open(&directory.0).unwrap();
    for n in 0..520 {
        log.append(Record::Data {
            offset: n * 65536,
            bytes: vec![65; 65536],
        })
        .unwrap();
    }
    assert!(log.first_offset() > 0);
    assert_eq!(log.next_offset(), 520 * 65536);
    let mut total = 0;
    for file in fs::read_dir(&directory.0).unwrap() {
        let size = file.unwrap().metadata().unwrap().len();
        assert!(size <= 512 * 1024);
        total += size;
    }
    assert!(total <= 32 * 1024 * 1024);
    let first = log.first_offset();
    drop(log);
    let log = Log::open(&directory.0).unwrap();
    assert_eq!(log.first_offset(), first);
    assert_eq!(
        log.read(first, first + 3, 3).unwrap(),
        vec![Record::Data {
            offset: first,
            bytes: vec![65; 3]
        }]
    );
}
