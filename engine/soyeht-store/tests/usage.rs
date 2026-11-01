#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use soyeht_protocol::{
    UsageActivity, UsageProvider, UsageReading, UsageSource, UsageState, UsageWindow,
};
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
        activity: None,
        source,
        fetched_at: "2026-09-12T18:00:00Z".into(),
    }
}

/// What a provider with no plan limit recorded: no window, only a count.
fn activity_only(tokens: i64, cost_usd: Option<f64>) -> UsageReading {
    UsageReading {
        primary: None,
        activity: Some(UsageActivity {
            label: "Last 7 days on this device".into(),
            tokens,
            cost_usd,
        }),
        ..reading(
            UsageProvider::Opencode,
            UsageSource::OpencodeLocalHistory,
            0.0,
        )
    }
}

fn save(store: &mut Store, item: &UsageReading, observed_at: i64, key: &str) {
    commit(store, key, |change| {
        change.save_usage_reading(item, observed_at)
    });
}

/// Whether the store refuses to save `item`, which it does before any SQL.
fn refuses(store: &mut Store, item: &UsageReading, observed_at: i64, key: &str) -> bool {
    let result = store.mutate("test", key, "bad", |change| {
        change.save_usage_reading(item, observed_at)?;
        Ok((json!({}), vec![]))
    });
    matches!(result, Err(Error::InvalidUsageReading))
}

fn stored(store: &Store, provider: &UsageProvider) -> UsageReading {
    store.usage_reading(provider).unwrap().unwrap().reading
}

fn schema_version(store: &Store) -> u32 {
    store
        .connection()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
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
        assert!(refuses(
            &mut store,
            &item,
            observed_at,
            &format!("bad-{index}")
        ));
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
    assert_eq!(stored(&store, &UsageProvider::Codex), item);
    assert_eq!(
        schema_version(&store),
        soyeht_store::database::SCHEMA_VERSION
    );
}

#[test]
fn an_activity_without_a_window_is_a_reading_and_survives_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    let item = activity_only(1_200_000, Some(3.4));
    save(&mut store, &item, 100, "activity");
    save(
        &mut store,
        &reading(
            UsageProvider::Opencode,
            UsageSource::OpencodeGoAllowance,
            0.2,
        ),
        200,
        "go",
    );
    save(&mut store, &activity_only(0, None), 300, "empty");
    drop(store);

    let stored = profile.store().usage_readings().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].reading, activity_only(0, None));
    assert_eq!(stored[0].observed_at_unix_sec, 300);
    assert_ne!(stored[0].reading, item);
}

#[test]
fn opencode_sources_belong_to_opencode_and_an_activity_must_be_a_count() {
    let profile = Profile::new();
    let mut store = profile.store();
    let wrong_provider = reading(UsageProvider::Cursor, UsageSource::OpencodeZaiPlan, 0.2);
    let negative = activity_only(-1, None);
    let negative_cost = activity_only(5, Some(-0.01));
    let mut unnamed = activity_only(5, None);
    if let Some(activity) = unnamed.activity.as_mut() {
        activity.label.clear();
    }
    let mut nothing = activity_only(5, None);
    nothing.activity = None;
    for (index, item) in [wrong_provider, negative, negative_cost, unnamed, nothing]
        .into_iter()
        .enumerate()
    {
        assert!(
            refuses(&mut store, &item, 1, &format!("bad-{index}")),
            "{index}"
        );
    }
    assert!(store.usage_readings().unwrap().is_empty());
}

#[test]
fn migration_from_v31_keeps_the_readings_and_accepts_opencode() {
    let profile = Profile::new();
    let mut store = profile.store();
    let kept = reading(UsageProvider::Codex, UsageSource::CodexAppServer, 0.3);
    save(&mut store, &kept, 400, "kept");
    drop(store);
    // The schema of version 31: the provider list of the table did not name OpenCode.
    profile
        .connection()
        .execute_batch(
            "ALTER TABLE usage_readings RENAME TO current_readings;
             CREATE TABLE usage_readings (
               provider TEXT PRIMARY KEY CHECK(provider IN ('claude','codex','cursor')),
               observed_at_unix_sec INTEGER NOT NULL CHECK(observed_at_unix_sec >= 0),
               payload TEXT NOT NULL CHECK(json_valid(payload))
             );
             INSERT INTO usage_readings SELECT * FROM current_readings;
             DROP TABLE current_readings;
             PRAGMA user_version = 31;",
        )
        .unwrap();

    let mut store = profile.store();
    assert_eq!(
        store
            .usage_reading(&UsageProvider::Codex)
            .unwrap()
            .unwrap()
            .reading,
        kept
    );
    save(&mut store, &activity_only(7, None), 500, "after");
    assert_eq!(store.usage_readings().unwrap().len(), 2);
    assert_eq!(
        schema_version(&store),
        soyeht_store::database::SCHEMA_VERSION
    );
}
