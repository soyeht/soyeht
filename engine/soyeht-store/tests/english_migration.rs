#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rusqlite::{Connection, params};
use serde_json::{Value, json};
use soyeht_protocol::{Pane, PaneState};
use soyeht_store::database::{SCHEMA_VERSION, Store};
use std::{fs, path::PathBuf};
use uuid::Uuid;

const STATES: [(&str, PaneState); 9] = [
    ("iniciando", PaneState::Starting),
    ("trabalhando", PaneState::Working),
    ("ociosa", PaneState::Idle),
    ("esperando_você", PaneState::WaitingForYou),
    ("cancelando", PaneState::Cancelling),
    ("sem_sinal", PaneState::NoSignal),
    ("recuperando", PaneState::Recovering),
    ("morta", PaneState::Dead),
    ("não_sei", PaneState::Unknown),
];

struct Profile(PathBuf);
impl Profile {
    fn v2() -> (Self, Connection) {
        let path = std::env::temp_dir().join(format!("soyeht-next-migration-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let connection = Connection::open(path.join("soyeht.db")).unwrap();
        connection
            .execute_batch(include_str!("../migrations/001-initial.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!("../migrations/002-client-state.sql"))
            .unwrap();
        connection.execute_batch("INSERT INTO metadata VALUES ('db_id','stable-db'),('engine_generation','41'); INSERT INTO workspaces(id,payload) VALUES ('workspace','{\"name\":\"ociosa\"}');").unwrap();
        (Self(path), connection)
    }
    fn backups(&self) -> Vec<PathBuf> {
        fs::read_dir(self.0.join("backups"))
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "db"))
            .collect()
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn old_pane(state: &str, index: usize) -> Value {
    json!({
        "paneId":format!("pane-{index}"), "workspaceId":"workspace", "name":state,
        "cwd":"/tmp", "mode":"terminal", "state":state, "source":"none", "confidence":0.0,
        "phrase":"Shell ativo", "generation":7, "streamId":format!("stream-{index}"),
        "cols":80, "rows":24, "record":true, "pid":1234,
        "future":{"state":state,"phrase":"Shell ativo"}
    })
}

fn insert_pane(connection: &Connection, index: usize, pane: &Value) {
    connection.execute(
        "INSERT INTO panes(id,workspace_id,payload,closed,instance_key,instance_id,supervisor_id,sequence_base) VALUES (?1,'workspace',?2,?3,?1,'instance','supervisor','123456')",
        params![format!("pane-{index}"),pane.to_string(),index == 7],
    ).unwrap();
}

fn scalar(connection: &Connection, sql: &str) -> String {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn populated_v2_preserves_identity_history_and_retries_while_converting_states() {
    let (profile, old) = Profile::v2();
    for (index, (state, _)) in STATES.iter().enumerate() {
        let pane = old_pane(state, index);
        insert_pane(&old, index, &pane);
        for method in [
            "pane.created",
            "pane.updated",
            "pane.renamed",
            "pane.state_changed",
        ] {
            let mut event = if method == "pane.state_changed" {
                json!({"paneId":pane["paneId"],"state":state,"phrase":"Shell ativo"})
            } else {
                json!({"pane":pane})
            };
            let id = index * 4
                + match method {
                    "pane.created" => 1,
                    "pane.updated" => 2,
                    "pane.renamed" => 3,
                    _ => 4,
                };
            event["eventId"] = json!(id.to_string());
            event["at"] = json!("2026-09-11T00:00:00Z");
            event["actor"] = json!({"role":"human_local","client":"test"});
            old.execute(
                "INSERT INTO events(id,method,params,created_unix) VALUES (?1,?2,?3,123)",
                params![i64::try_from(id).unwrap(), method, event.to_string()],
            )
            .unwrap();
        }
        old.execute("INSERT INTO idempotency(scope,key,fingerprint,result,created_unix,actor) VALUES ('test',?1,'stable-fingerprint',?2,123,'{}')",params![format!("key-{index}"),json!({"pane":pane}).to_string()]).unwrap();
    }
    let user_text = r#"{"state":"ociosa","phrase":"Shell ativo","pane":{"paneId":"user","workspaceId":"text","state":"morta"}}"#;
    old.execute("INSERT INTO settings VALUES ('user-text',?1)", [user_text])
        .unwrap();
    old.execute(
        "INSERT INTO events(method,params,created_unix) VALUES ('unrelated',?1,123)",
        [user_text],
    )
    .unwrap();
    old.execute("INSERT INTO idempotency(scope,key,fingerprint,result,created_unix) VALUES ('test','user-text','unchanged',?1,123)",[json!({"value":serde_json::from_str::<Value>(user_text).unwrap()}).to_string()]).unwrap();
    old.execute(
        "INSERT INTO client_focus(scope,workspace_id,pane_id) VALUES ('test','workspace','pane-1')",
        [],
    )
    .unwrap();
    drop(old);

    let mut store = Store::open(&profile.0).unwrap();
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    assert_eq!(store.db_id, "stable-db");
    assert_eq!(store.next_engine_generation().unwrap(), 42);
    assert_eq!(store.snapshot().unwrap().cursor, 37);
    assert_eq!(
        scalar(
            store.connection(),
            "SELECT value FROM settings WHERE key='user-text'"
        ),
        user_text
    );
    assert_eq!(
        scalar(store.connection(), "SELECT payload FROM workspaces"),
        r#"{"name":"ociosa"}"#
    );
    assert_eq!(
        scalar(
            store.connection(),
            "SELECT params FROM events WHERE method='unrelated'"
        ),
        user_text
    );
    assert_eq!(
        scalar(store.connection(), "SELECT pane_id FROM client_focus"),
        "pane-1"
    );
    let (_, events) = store.replay(0).unwrap();
    for (index, (old_state, expected)) in STATES.iter().enumerate() {
        let raw: String = store
            .connection()
            .query_row(
                "SELECT payload FROM panes WHERE id=?1",
                [format!("pane-{index}")],
                |r| r.get(0),
            )
            .unwrap();
        let actual: Value = serde_json::from_str(&raw).unwrap();
        let mut wanted = old_pane(old_state, index);
        wanted["state"] = serde_json::to_value(expected).unwrap();
        wanted["phrase"] = json!("Shell active");
        assert_eq!(actual, wanted);
        assert_eq!(
            serde_json::from_value::<Pane>(actual.clone())
                .unwrap()
                .state,
            *expected
        );
        let process:(bool,String,String,String,String)=store.connection().query_row("SELECT closed,instance_key,instance_id,supervisor_id,sequence_base FROM panes WHERE id=?1",[format!("pane-{index}")],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        assert_eq!(
            process,
            (
                index == 7,
                format!("pane-{index}"),
                "instance".into(),
                "supervisor".into(),
                "123456".into()
            )
        );
        for event in &events[index * 4..index * 4 + 4] {
            assert_eq!(event.params["eventId"], event.id.to_string());
            assert_eq!(event.params["actor"]["client"], "test");
            if event.method == "pane.state_changed" {
                assert_eq!(event.params["state"], wanted["state"]);
            } else {
                assert_eq!(event.params["pane"], wanted);
            }
        }
        let replay = store
            .mutate(
                "test",
                &format!("key-{index}"),
                "stable-fingerprint",
                |_| panic!("old mutation ran again"),
            )
            .unwrap();
        assert!(replay.repeated);
        assert_eq!(replay.result, json!({"pane":wanted}));
    }
    let receipt = store
        .mutate("test", "user-text", "unchanged", |_| {
            panic!("user operation repeated")
        })
        .unwrap();
    assert_eq!(
        receipt.result["value"],
        serde_json::from_str::<Value>(user_text).unwrap()
    );
    let backups = profile.backups();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(&backups[0]).unwrap();
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        serde_json::from_str::<Value>(&scalar(
            &backup,
            "SELECT payload FROM panes WHERE id='pane-0'"
        ))
        .unwrap()["state"],
        "iniciando"
    );
    drop(store);
    let mut reopened = Store::open(&profile.0).unwrap();
    assert_eq!(reopened.snapshot().unwrap().cursor, 37);
    assert_eq!(reopened.db_id, "stable-db");
    assert_eq!(profile.backups().len(), 1);
}

#[test]
fn malformed_history_rolls_back_all_values_and_the_version() {
    let (profile, old) = Profile::v2();
    let pane = old_pane("iniciando", 0);
    insert_pane(&old, 0, &pane);
    old.execute(
        "INSERT INTO events(method,params,created_unix) VALUES ('pane.updated','invalid JSON',123)",
        [],
    )
    .unwrap();
    drop(old);
    assert!(Store::open(&profile.0).is_err());
    let unchanged = Connection::open(profile.0.join("soyeht.db")).unwrap();
    assert_eq!(
        unchanged
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        serde_json::from_str::<Value>(&scalar(&unchanged, "SELECT payload FROM panes")).unwrap(),
        pane
    );
    assert_eq!(profile.backups().len(), 1);
}
