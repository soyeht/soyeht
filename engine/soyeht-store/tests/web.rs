#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::{
    Pane, PaneMode, PaneState, StateSource, WebSessionState, WebViewport, WebViewportPreset,
    Workspace,
};
use soyeht_store::{
    Error,
    database::Store,
    domain::PaneRecord,
    web::{WebSessionGrant, WebSessionGrantScope},
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-web-store-{}", Uuid::new_v4()));
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

fn workspace() -> Workspace {
    Workspace {
        workspace_id: "workspace-1".into(),
        name: "Web".into(),
        path: "/tmp/web".into(),
        layout: json!({}),
    }
}

fn web_pane() -> PaneRecord {
    PaneRecord {
        pane: Pane {
            pane_id: "web-pane-1".into(),
            workspace_id: "workspace-1".into(),
            name: "Local app".into(),
            cwd: "/tmp/web".into(),
            mode: PaneMode::Web,
            state: PaneState::Idle,
            source: StateSource::Protocol,
            confidence: 1.0,
            phrase: "ready".into(),
            generation: 1,
            cols: 80,
            rows: 24,
            record: false,
            pid: None,
            stream_id: None,
            agent: None,
            url: Some("https://example.test/path".into()),
            title: Some("Local app".into()),
            viewport: Some(WebViewport {
                width: 1280,
                height: 800,
                preset: WebViewportPreset::Responsive,
                zoom_percent: 100,
            }),
            incognito: Some(true),
            web_session: Some(WebSessionState::Incognito),
        },
        closed: false,
        instance_key: "web:web-pane-1".into(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    }
}

fn grant(scope: WebSessionGrantScope, actor: &str, created_ms: i64) -> WebSessionGrant {
    WebSessionGrant {
        workspace_id: "workspace-1".into(),
        domain: "example.test".into(),
        scope,
        granted_by: actor.into(),
        created_ms,
    }
}

fn seed_workspace(store: &mut Store) {
    store
        .mutate("test", "workspace", "workspace", |change| {
            change.insert_workspace(&workspace())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn seed_task(store: &Store) {
    store
        .connection()
        .execute(
            "INSERT INTO tasks(workspace_id,task_id,state,priority_rank,version,payload) VALUES ('workspace-1','WS-1','in_progress',2,1,'{}')",
            [],
        )
        .unwrap();
}

fn save_grant(store: &mut Store, item: &WebSessionGrant, key: &str) {
    store
        .mutate("test", key, key, |change| {
            change.save_web_session_grant(item)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

#[test]
fn web_pane_metadata_and_scoped_session_grants_survive_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    seed_task(&store);
    store
        .mutate("test", "pane", "pane", |change| {
            change.insert_pane(&web_pane())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    save_grant(
        &mut store,
        &grant(WebSessionGrantScope::Workspace, "mac", 100),
        "workspace-grant",
    );
    save_grant(
        &mut store,
        &grant(WebSessionGrantScope::Task("WS-1".into()), "iphone", 200),
        "task-grant",
    );
    drop(store);

    let store = profile.store();
    let pane = store
        .panes()
        .unwrap()
        .into_iter()
        .find(|item| item.pane.pane_id == "web-pane-1")
        .unwrap();
    assert_eq!(pane.pane, web_pane().pane);
    assert_eq!(
        store
            .web_session_grant("workspace-1", "example.test", Some("WS-1"))
            .unwrap()
            .unwrap(),
        grant(WebSessionGrantScope::Task("WS-1".into()), "iphone", 200)
    );
    assert_eq!(
        store
            .web_session_grant("workspace-1", "example.test", Some("WS-2"))
            .unwrap()
            .unwrap(),
        grant(WebSessionGrantScope::Workspace, "mac", 100)
    );
    assert_eq!(store.web_session_grants("workspace-1").unwrap().len(), 2);
}

#[test]
fn grants_replace_and_revoke_atomically_without_broadening_scope() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    seed_task(&store);
    let scope = WebSessionGrantScope::Task("WS-1".into());
    save_grant(&mut store, &grant(scope.clone(), "mac", 100), "first");
    save_grant(&mut store, &grant(scope.clone(), "iphone", 200), "replace");
    let stored = store
        .web_session_grant("workspace-1", "example.test", Some("WS-1"))
        .unwrap()
        .unwrap();
    assert_eq!(stored.granted_by, "iphone");
    assert_eq!(stored.created_ms, 200);
    assert!(
        store
            .web_session_grant("workspace-1", "example.test", None)
            .unwrap()
            .is_none()
    );

    let removed = store
        .mutate("test", "remove", "remove", |change| {
            let removed = change.remove_web_session_grant("workspace-1", "example.test", &scope)?;
            Ok((json!({"removed": removed}), vec![]))
        })
        .unwrap();
    assert_eq!(removed.result, json!({"removed": true}));
    assert!(store.web_session_grants("workspace-1").unwrap().is_empty());
}

#[test]
fn invalid_grants_fail_before_sql_and_private_page_data_has_no_schema_surface() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    for (index, item) in [
        grant(WebSessionGrantScope::Workspace, "", 1),
        grant(WebSessionGrantScope::Workspace, "mac", -1),
        WebSessionGrant {
            domain: "HTTPS://example.test/path".into(),
            ..grant(WebSessionGrantScope::Workspace, "mac", 1)
        },
        grant(WebSessionGrantScope::Task(String::new()), "mac", 1),
    ]
    .into_iter()
    .enumerate()
    {
        let result = store.mutate("test", &format!("bad-{index}"), "bad", |change| {
            change.save_web_session_grant(&item)?;
            Ok((json!({}), vec![]))
        });
        assert!(matches!(result, Err(Error::InvalidWebSessionGrant)));
    }
    assert!(store.web_session_grants("workspace-1").unwrap().is_empty());

    let schema: String = store
        .connection()
        .query_row(
            "SELECT group_concat(lower(name || ' ' || sql), ' ') FROM sqlite_schema WHERE sql IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for private_state in [
        "cookie",
        "screenshot",
        "console",
        "network",
        "command_waiter",
        "host_connection",
    ] {
        assert!(
            !schema.contains(private_state),
            "private state entered SQLite: {private_state}"
        );
    }
}

#[test]
fn migration_from_populated_v15_preserves_rows_and_adds_web_grants() {
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
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            r#"INSERT INTO metadata(key,value) VALUES ('db_id','v15-db'),('engine_generation','0');
               INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1','{"workspaceId":"workspace-1","name":"Web","path":"/tmp/web","layout":{}}',0);
               INSERT INTO settings(key,value) VALUES ('app-bar-open','true');"#,
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(store.workspace("workspace-1").unwrap().unwrap().name, "Web");
    assert_eq!(store.setting("app-bar-open").unwrap(), Some(json!(true)));
    save_grant(
        &mut store,
        &grant(WebSessionGrantScope::Workspace, "mac", 100),
        "after-migration",
    );
    assert_eq!(store.web_session_grants("workspace-1").unwrap().len(), 1);
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}
