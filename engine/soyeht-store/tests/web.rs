#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::json;
use soyeht_protocol::{
    DecisionBy, Pane, PaneMode, PanePermissionAnswerResult, PermissionOption, PermissionOptionKind,
    PermissionRequest, PermissionRequestKind, PermissionResult, WebSessionState, WebViewport,
    WebViewportPreset, Workspace,
};
use soyeht_store::{
    Error,
    database::Store,
    domain::PaneRecord,
    web::{WebPermissionRequest, WebSessionGrant, WebSessionGrantScope},
};
use support::{Profile, apply_migrations, commit};

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
            name: "Local app".into(),
            phrase: "ready".into(),
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
            ..support::pane("web-pane-1", "workspace-1", PaneMode::Web, "/tmp/web")
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

fn permission(request_id: &str) -> WebPermissionRequest {
    WebPermissionRequest {
        request_id: request_id.into(),
        pane_id: "web-pane-1".into(),
        workspace_id: "workspace-1".into(),
        generation: 1,
        task_id: Some("WS-1".into()),
        kind: PermissionRequestKind::WebSession,
        domain: "example.test".into(),
        public: PermissionRequest {
            request_id: request_id.into(),
            pane_id: "web-pane-1".into(),
            generation: 1,
            prompt_id: "web-operation-1".into(),
            title: "Use signed-in session?".into(),
            detail: "The page has a signed-in session for example.test.".into(),
            presentation: None,
            options: vec![PermissionOption {
                option_id: "task".into(),
                kind: PermissionOptionKind::AllowOnce,
                name: "This task only".into(),
            }],
            expires_at: "2026-09-13T18:00:00.000Z".into(),
            kind: Some(PermissionRequestKind::WebSession),
            domain: Some("example.test".into()),
        },
        expires_ms: 1_000,
        result: None,
    }
}

fn answer(request_id: &str, by: DecisionBy) -> PanePermissionAnswerResult {
    PanePermissionAnswerResult {
        request_id: request_id.into(),
        result: PermissionResult::Selected,
        option_id: Some("task".into()),
        by,
    }
}

fn seed_workspace(store: &mut Store) {
    commit(store, "workspace", |change| {
        change.insert_workspace(&workspace())
    });
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
    commit(store, key, |change| change.save_web_session_grant(item));
}

#[test]
fn web_pane_metadata_and_scoped_session_grants_survive_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    seed_task(&store);
    commit(&mut store, "pane", |change| change.insert_pane(&web_pane()));
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
}

#[test]
fn pending_web_permission_survives_reopen_enters_snapshot_and_accepts_one_answer() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    seed_task(&store);
    commit(&mut store, "pane", |change| change.insert_pane(&web_pane()));
    let pending = permission("request-1");
    commit(&mut store, "permission", |change| {
        change.save_web_permission_request(&pending)
    });
    drop(store);

    let mut store = profile.store();
    assert_eq!(
        store
            .open_web_permission(
                "web-pane-1",
                &PermissionRequestKind::WebSession,
                "example.test",
                Some("WS-1"),
            )
            .unwrap()
            .unwrap()
            .request_id,
        "request-1"
    );
    let snapshot = store.snapshot().unwrap();
    let requests = &snapshot
        .sections
        .iter()
        .find(|(name, _)| name == "requests")
        .unwrap()
        .1;
    assert_eq!(requests[0]["request"]["requestId"], "request-1");

    let first = answer("request-1", DecisionBy::Mac);
    commit(&mut store, "answer", |change| {
        assert!(change.answer_web_permission("request-1", &first)?);
        Ok(())
    });
    let second = answer("request-1", DecisionBy::Iphone);
    commit(&mut store, "second-answer", |change| {
        assert!(!change.answer_web_permission("request-1", &second)?);
        Ok(())
    });
    let stored = store.web_permission_request("request-1").unwrap().unwrap();
    assert_eq!(stored.result.unwrap().by, DecisionBy::Mac);
    let snapshot = store.snapshot().unwrap();
    let requests = snapshot
        .sections
        .iter()
        .find(|(name, _)| name == "requests")
        .unwrap()
        .1
        .as_array()
        .unwrap();
    assert!(requests.is_empty());
}

#[test]
fn permission_operation_identity_and_expiry_are_durable_and_bounded() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    seed_task(&store);
    commit(&mut store, "pane", |change| change.insert_pane(&web_pane()));
    let pending = permission("operation-request");
    commit(&mut store, "permission", |change| {
        change.save_web_permission_request(&pending)
    });
    drop(store);

    let store = profile.store();
    let restored = store
        .web_permission_for_operation(
            "web-pane-1",
            &PermissionRequestKind::WebSession,
            "web-operation-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(restored.request_id, "operation-request");
    assert!(
        store
            .open_web_permission(
                "web-pane-1",
                &PermissionRequestKind::WebSession,
                "example.test",
                Some("WS-1"),
            )
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .open_web_permission(
                "web-pane-1",
                &PermissionRequestKind::WebSession,
                "example.test",
                None,
            )
            .unwrap()
            .is_none()
    );
    assert!(store.expired_web_permissions(999).unwrap().is_empty());
    assert_eq!(store.expired_web_permissions(1_000).unwrap().len(), 1);
    assert!(matches!(
        store.expired_web_permissions(-1),
        Err(Error::InvalidWebPermission)
    ));
}

#[test]
fn web_permission_identity_is_validated_before_sql() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace(&mut store);
    seed_task(&store);
    commit(&mut store, "pane", |change| change.insert_pane(&web_pane()));
    for (index, mut invalid) in [
        permission("bad-1"),
        permission("bad-2"),
        permission("bad-3"),
    ]
    .into_iter()
    .enumerate()
    {
        match index {
            0 => invalid.public.pane_id = "another-pane".into(),
            1 => invalid.kind = PermissionRequestKind::Agent,
            _ => invalid.domain = "HTTPS://example.test".into(),
        }
        let result = store.mutate("test", &format!("bad-{index}"), "bad", |change| {
            change.save_web_permission_request(&invalid)?;
            Ok((json!({}), vec![]))
        });
        let error = result.err();
        assert!(
            matches!(error, Some(Error::InvalidWebPermission)),
            "unexpected validation error at case {index}: {error:?}"
        );
    }
    assert!(
        store
            .open_web_permission(
                "web-pane-1",
                &PermissionRequestKind::WebSession,
                "example.test",
                None,
            )
            .unwrap()
            .is_none()
    );
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
    let connection = profile.connection();
    apply_migrations(&connection, 15);
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

#[test]
fn migration_from_populated_v16_preserves_web_state_and_adds_pending_permissions() {
    let profile = Profile::new();
    let connection = profile.connection();
    apply_migrations(&connection, 16);
    connection
        .execute(
            "INSERT INTO metadata(key,value) VALUES ('db_id','v16-db')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workspaces(id,payload,closed) VALUES (?1,?2,0)",
            rusqlite::params!["workspace-1", serde_json::to_string(&workspace()).unwrap()],
        )
        .unwrap();
    let pane = web_pane();
    connection
        .execute(
            "INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES (?1,?2,?3,0,?4,'0')",
            rusqlite::params![pane.pane.pane_id,pane.pane.workspace_id,serde_json::to_string(&pane.pane).unwrap(),pane.instance_key],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO web_session_grants(workspace_id,domain,scope,task_id,granted_by,created_ms) VALUES ('workspace-1','example.test','workspace',NULL,'mac',100)",
            [],
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(store.web_session_grants("workspace-1").unwrap().len(), 1);
    let mut pending = permission("after-v16");
    pending.task_id = None;
    commit(&mut store, "pending", |change| {
        change.save_web_permission_request(&pending)
    });
    assert!(store.web_permission_request("after-v16").unwrap().is_some());
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}

#[test]
fn migration_from_populated_v17_preserves_permissions_and_adds_operation_identity() {
    let profile = Profile::new();
    let database = profile.0.join("soyeht.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    apply_migrations(&connection, 17);
    connection
        .execute(
            "INSERT INTO metadata(key,value) VALUES ('db_id','v17-db')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workspaces(id,payload,closed) VALUES ('workspace-1',?1,0)",
            [serde_json::to_string(&workspace()).unwrap()],
        )
        .unwrap();
    let pane = web_pane();
    connection
        .execute(
            "INSERT INTO panes(id,workspace_id,payload,closed,instance_key,sequence_base) VALUES (?1,?2,?3,0,?4,'0')",
            rusqlite::params![pane.pane.pane_id, pane.pane.workspace_id,
                serde_json::to_string(&pane.pane).unwrap(), pane.instance_key],
        )
        .unwrap();
    let pending = permission("v17-request");
    connection
        .execute(
            "INSERT INTO web_permission_requests(request_id,pane_id,workspace_id,generation,task_id,kind,domain,public,expires_ms,result) VALUES (?1,?2,?3,?4,NULL,'web_session',?5,?6,?7,NULL)",
            rusqlite::params![pending.request_id, pending.pane_id, pending.workspace_id,
                pending.generation, pending.domain,
                serde_json::to_string(&pending.public).unwrap(), pending.expires_ms],
        )
        .unwrap();
    drop(connection);

    let store = profile.store();
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
    assert!(
        store
            .web_permission_for_operation(
                "web-pane-1",
                &PermissionRequestKind::WebSession,
                "web-operation-1"
            )
            .unwrap()
            .is_some()
    );
}
