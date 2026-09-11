use rusqlite::params;
use serde_json::json;
use soyeht_store::{
    database::{Error, Store},
    segments::{Log, Record},
};
use std::{fs, io::Write, path::PathBuf};
use uuid::Uuid;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("soyeht-next-store-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
        .mutate("local:test", "key-1", "request-1", |tx| {
            tx.execute(
                "INSERT INTO workspaces(id,payload) VALUES (?1,?2)",
                params!["w1", json!({"workspaceId":"w1"}).to_string()],
            )?;
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
    let failed = store.mutate("local:test", "key-1", "request", |tx| {
        tx.execute("INSERT INTO workspaces(id,payload) VALUES ('w1','{}')", [])?;
        Err(Error::Invalid("injected failure"))
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
        1
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
    let result = store.mutate("local:test", "large", "large-request", |tx| {
        tx.execute(
            "INSERT INTO workspaces(id,payload) VALUES ('large',?1)",
            ["x".repeat(2 * 1024 * 1024)],
        )?;
        Ok((json!({}), vec![("workspace.created".into(), json!({}))]))
    });
    assert!(
        matches!(result,Err(Error::Sql(rusqlite::Error::SqliteFailure(error,_))) if error.code == rusqlite::ErrorCode::DiskFull)
    );
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
