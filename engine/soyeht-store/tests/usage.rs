#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::{UsageProvider, UsageReading, UsageSource, UsageState, UsageWindow};
use soyeht_store::{Error, database::Store};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-usage-store-{}", Uuid::new_v4()));
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

fn reading(provider: UsageProvider, source: UsageSource, used: f64) -> UsageReading {
    UsageReading {
        provider,
        state: UsageState::Ok,
        unavailable_reason: None,
        primary: Some(UsageWindow {
            used,
            resets_at: "2026-09-13T00:00:00Z".into(),
            label: "Current window".into(),
        }),
        secondary: None,
        source,
        fetched_at: "2026-09-12T18:00:00Z".into(),
    }
}

fn save(store: &mut Store, item: &UsageReading, observed_at: i64, key: &str) {
    store
        .mutate("test", key, key, |change| {
            change.save_usage_reading(item, observed_at)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

#[test]
fn one_last_reading_per_provider_replaces_atomically_and_survives_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    let first = reading(UsageProvider::Claude, UsageSource::ClaudeCliUsage, 0.2);
    let replacement = reading(UsageProvider::Claude, UsageSource::ClaudeCliUsage, 0.4);
    let other = reading(UsageProvider::Cursor, UsageSource::CursorCurrentPeriod, 0.6);
    save(&mut store, &first, 100, "first");
    save(&mut store, &replacement, 200, "replacement");
    save(&mut store, &other, 300, "other");
    drop(store);

    let store = profile.store();
    let all = store.usage_readings().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].reading, replacement);
    assert_eq!(all[0].observed_at_unix_sec, 200);
    assert_eq!(all[1].reading, other);
    assert_eq!(store.usage_reading(&UsageProvider::Codex).unwrap(), None);
}

#[test]
fn invalid_source_state_window_and_timestamp_are_rejected_before_sql() {
    let profile = Profile::new();
    let mut store = profile.store();
    let wrong_source = reading(UsageProvider::Claude, UsageSource::CursorCurrentPeriod, 0.2);
    let mut missing_window = reading(UsageProvider::Codex, UsageSource::CodexAppServer, 0.2);
    missing_window.primary = None;
    let invalid_fraction = reading(UsageProvider::Cursor, UsageSource::CursorCurrentPeriod, 1.1);
    for (index, (item, observed_at)) in [
        (wrong_source, 1),
        (missing_window, 1),
        (invalid_fraction, 1),
        (
            reading(UsageProvider::Cursor, UsageSource::CursorCurrentPeriod, 0.1),
            -1,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let result = store.mutate("test", &format!("bad-{index}"), "bad", |change| {
            change.save_usage_reading(&item, observed_at)?;
            Ok((json!({}), vec![]))
        });
        assert!(matches!(result, Err(Error::InvalidUsageReading)));
    }
    assert!(store.usage_readings().unwrap().is_empty());
}

#[test]
fn migration_from_populated_v10_preserves_existing_rows_and_adds_usage() {
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
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v10-db'),('engine_generation','0');
             INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"One","path":"/tmp/one","layout":{}}',0);
             INSERT INTO settings(key,value) VALUES ('rail-open','true');"#,
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(store.workspace("workspace-1").unwrap().unwrap().name, "One");
    assert_eq!(store.setting("rail-open").unwrap(), Some(json!(true)));
    assert!(store.usage_readings().unwrap().is_empty());
    let item = reading(UsageProvider::Codex, UsageSource::CodexAppServer, 0.3);
    save(&mut store, &item, 400, "after-migration");
    assert_eq!(
        store
            .usage_reading(&UsageProvider::Codex)
            .unwrap()
            .unwrap()
            .reading,
        item
    );
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}
