#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use soyeht_protocol::{UsageProvider, UsageReading, UsageSource, UsageState, UsageWindow};
use soyeht_store::{Error, database::Store};
use support::{Profile, apply_migrations, commit};

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
    commit(store, key, |change| {
        change.save_usage_reading(item, observed_at)
    });
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
fn every_chain_source_stores_and_a_window_may_lack_a_reset_time() {
    let profile = Profile::new();
    let mut store = profile.store();
    let sources = [
        (UsageProvider::Claude, UsageSource::ClaudeOauthEndpoint),
        (UsageProvider::Claude, UsageSource::ClaudeCliUsage),
        (UsageProvider::Claude, UsageSource::ClaudeWeb),
        (UsageProvider::Codex, UsageSource::CodexAccessToken),
        (UsageProvider::Codex, UsageSource::CodexAccountEndpoint),
        (UsageProvider::Codex, UsageSource::CodexAppServer),
        (UsageProvider::Cursor, UsageSource::CursorAppLogin),
        (UsageProvider::Cursor, UsageSource::CursorBrowserCookies),
    ];
    for (index, (provider, source)) in sources.into_iter().enumerate() {
        let mut item = reading(provider, source, 0.5);
        if let Some(window) = item.primary.as_mut() {
            window.resets_at.clear();
        }
        save(&mut store, &item, 100, &format!("source-{index}"));
    }
    assert_eq!(store.usage_readings().unwrap().len(), 3);
}

#[test]
fn migration_from_populated_v10_preserves_existing_rows_and_adds_usage() {
    let profile = Profile::new();
    let connection = profile.connection();
    apply_migrations(&connection, 10);
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
