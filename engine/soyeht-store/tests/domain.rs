#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use soyeht_protocol::{
    AttentionItem, Pane, PaneMode, PaneState, ProbeCapability, ProbeResult, StateSource, Workspace,
};
use soyeht_store::{
    agents::{self, Outgoing},
    database::Store,
    domain::{InputIntent, PaneRecord},
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-domain-store-{}", Uuid::new_v4()));
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

fn workspace(name: &str) -> Workspace {
    Workspace {
        workspace_id: "w".into(),
        name: name.into(),
        path: "/tmp/project".into(),
        layout: json!({"direction":"row"}),
    }
}

fn workspace_with_id(id: &str) -> Workspace {
    Workspace {
        workspace_id: id.into(),
        name: id.into(),
        path: format!("/tmp/{id}"),
        layout: json!({"direction":"row"}),
    }
}

fn pane() -> PaneRecord {
    PaneRecord {
        pane: Pane {
            pane_id: "p".into(),
            workspace_id: "w".into(),
            name: "Agent".into(),
            cwd: "/tmp/project".into(),
            mode: PaneMode::Agent,
            state: PaneState::Idle,
            source: StateSource::Protocol,
            confidence: 1.0,
            phrase: String::new(),
            generation: 1,
            cols: 80,
            rows: 24,
            record: true,
            experiment_capture: None,
            pid: None,
            stream_id: None,
            agent: None,
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            writing_kind: None,
            writing_section: None,
            web_session: None,
        },
        closed: false,
        instance_key: "instance-key".into(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    }
}

fn seed_workspace_and_pane(store: &mut Store) {
    store
        .mutate("test", "seed", "seed", |change| {
            change.insert_workspace(&workspace("Original"))?;
            change.insert_pane(&pane())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

#[test]
fn workspaces_keep_creation_order_across_updates_and_reopen() {
    let profile = Profile::new();
    {
        let mut store = profile.store();
        store
            .mutate("test", "seed-order", "seed-order", |change| {
                change.insert_workspace(&workspace_with_id("z-last-alphabetically"))?;
                change.insert_workspace(&workspace_with_id("a-first-alphabetically"))?;
                change.insert_workspace(&workspace_with_id("m-middle"))?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
        store
            .mutate("test", "update-order", "update-order", |change| {
                let mut updated = workspace_with_id("a-first-alphabetically");
                updated.name = "Renamed without moving".into();
                change.update_workspace(&updated)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
        let sequences = store
            .connection()
            .prepare("SELECT id,created_seq FROM workspaces ORDER BY created_seq")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            sequences,
            [
                ("z-last-alphabetically".into(), 1),
                ("a-first-alphabetically".into(), 2),
                ("m-middle".into(), 3)
            ]
        );
        for (workspace_id, rowid) in [
            ("z-last-alphabetically", -30),
            ("a-first-alphabetically", -20),
            ("m-middle", -10),
        ] {
            store
                .connection()
                .execute(
                    "UPDATE workspaces SET rowid=?1 WHERE id=?2",
                    rusqlite::params![rowid, workspace_id],
                )
                .unwrap();
        }
        assert_eq!(
            store
                .workspaces()
                .unwrap()
                .iter()
                .map(|workspace| workspace.workspace_id.as_str())
                .collect::<Vec<_>>(),
            [
                "z-last-alphabetically",
                "a-first-alphabetically",
                "m-middle"
            ]
        );
    }

    let mut reopened = profile.store();
    assert_eq!(
        reopened
            .workspaces()
            .unwrap()
            .iter()
            .map(|workspace| workspace.workspace_id.as_str())
            .collect::<Vec<_>>(),
        [
            "z-last-alphabetically",
            "a-first-alphabetically",
            "m-middle"
        ]
    );
    let snapshot = reopened.snapshot().unwrap();
    let snapshot_workspaces = snapshot
        .sections
        .iter()
        .find(|(name, _)| name == "workspaces")
        .unwrap()
        .1
        .as_array()
        .unwrap();
    assert_eq!(
        snapshot_workspaces
            .iter()
            .map(|workspace| workspace["workspaceId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "z-last-alphabetically",
            "a-first-alphabetically",
            "m-middle"
        ]
    );
}

#[test]
fn panes_keep_creation_order_across_updates_rowid_changes_and_snapshot() {
    // Clients lay out automatic workspaces in pane order, so a snapshot that
    // sorted panes by identifier could swap two panes after a reconnect.
    let ids = ["z-created-first", "a-created-second", "m-created-third"];
    let pane_with_id = |id: &str| {
        let mut record = pane();
        record.pane.pane_id = id.into();
        record.instance_key = format!("instance-{id}");
        record
    };
    let profile = Profile::new();
    {
        let mut store = profile.store();
        store
            .mutate("test", "seed-panes", "seed-panes", |change| {
                change.insert_workspace(&workspace("Original"))?;
                for id in ids {
                    change.insert_pane(&pane_with_id(id))?;
                }
                Ok((json!({}), vec![]))
            })
            .unwrap();
        store
            .mutate("test", "update-pane", "update-pane", |change| {
                let mut updated = pane_with_id("a-created-second");
                updated.pane.state = PaneState::WaitingForYou;
                change.update_pane(&updated)?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
        for (id, rowid) in ids.iter().zip([30, 20, 10]) {
            store
                .connection()
                .execute(
                    "UPDATE panes SET rowid=?1 WHERE id=?2",
                    rusqlite::params![rowid, id],
                )
                .unwrap();
        }
    }
    let mut reopened = profile.store();
    assert_eq!(
        reopened
            .panes()
            .unwrap()
            .iter()
            .map(|record| record.pane.pane_id.as_str())
            .collect::<Vec<_>>(),
        ids
    );
    let snapshot = reopened.snapshot().unwrap();
    let snapshot_panes = snapshot
        .sections
        .iter()
        .find(|(name, _)| name == "panes")
        .unwrap()
        .1
        .as_array()
        .unwrap();
    assert_eq!(
        snapshot_panes
            .iter()
            .map(|pane| pane["paneId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn workspace_pane_settings_launch_and_focus_operations_round_trip() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace_and_pane(&mut store);
    store
        .mutate("test", "configure", "configure", |change| {
            let mut changed_workspace = workspace("Renamed");
            changed_workspace.layout = json!({"direction":"column"});
            change.update_workspace(&changed_workspace)?;
            let mut changed_pane = pane();
            changed_pane.pane.name = "Restored agent".into();
            changed_pane.instance_id = Some("instance-1".into());
            changed_pane.supervisor_id = Some("supervisor-1".into());
            changed_pane.sequence_base = 42;
            change.update_pane(&changed_pane)?;
            change.insert_pane_launch("p", &json!({"command":"sign-in"}))?;
            change.set_setting("appearance", &json!({"theme":"dark"}))?;
            change.focus_pane("client", "w", "p")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();

    assert_eq!(store.workspace("w").unwrap().unwrap().name, "Renamed");
    let stored_pane = store.panes().unwrap().remove(0);
    assert_eq!(stored_pane.pane.name, "Restored agent");
    assert_eq!(stored_pane.instance_id.as_deref(), Some("instance-1"));
    assert_eq!(stored_pane.sequence_base, 42);
    assert_eq!(
        store.setting("appearance").unwrap(),
        Some(json!({"theme":"dark"}))
    );
    assert!(store.has_pane_launch("p").unwrap());
    assert_eq!(
        store.pane_launch::<Value>("p").unwrap(),
        Some(json!({"command":"sign-in"}))
    );
    let focus = store.client_focus("client").unwrap().unwrap();
    assert_eq!(focus.workspace_id.as_deref(), Some("w"));
    assert_eq!(focus.pane_id.as_deref(), Some("p"));

    store
        .mutate("test", "close-pane", "close-pane", |change| {
            change.close_pane("p")?;
            change.focus_workspace("client", "w")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.panes().unwrap()[0].closed);
    assert_eq!(store.client_focus("client").unwrap().unwrap().pane_id, None);
    store
        .mutate("test", "close-workspace", "close-workspace", |change| {
            change.close_workspace("w")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.workspace("w").unwrap().is_none());
    assert_eq!(
        store.client_focus("client").unwrap().unwrap().workspace_id,
        None
    );
}

#[test]
fn input_intent_and_idempotency_operations_keep_their_durable_identity() {
    let profile = Profile::new();
    let mut store = profile.store();
    let intent = InputIntent {
        fingerprint: "input-fingerprint".into(),
        operation_key: "operation-1".into(),
        supervisor_id: "supervisor-1".into(),
        instance_id: "instance-1".into(),
    };
    store.record_input_intent("client", "key", &intent).unwrap();
    assert_eq!(store.input_intent("client", "key").unwrap(), Some(intent));
    assert!(store.input_intent_operation_pending("operation-1").unwrap());

    store
        .mutate("client", "key", "input-fingerprint", |change| {
            change.remove_input_intent("client", "key")?;
            Ok((json!({"status":"pending"}), vec![]))
        })
        .unwrap();
    assert!(store.input_intent("client", "key").unwrap().is_none());
    assert!(!store.input_intent_operation_pending("operation-1").unwrap());
    let receipt = store.idempotency_receipt("client", "key").unwrap().unwrap();
    assert_eq!(receipt.fingerprint, "input-fingerprint");
    assert_eq!(receipt.result, json!({"status":"pending"}));

    store
        .mutate("system", "complete", "complete", |change| {
            assert!(change.update_idempotency_result(
                "client",
                "key",
                &json!({"status":"delivered"}),
            )?);
            assert!(!change.update_idempotency_result("missing", "key", &json!({}))?);
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store
            .idempotency_receipt("client", "key")
            .unwrap()
            .unwrap()
            .result,
        json!({"status":"delivered"})
    );
}

#[test]
fn pairing_operations_expire_requests_and_bind_then_revoke_a_device() {
    let profile = Profile::new();
    let mut store = profile.store();
    store
        .mutate("test", "pair", "pair", |change| {
            change.open_pair_window("123456", 100, 0)?;
            change.insert_pair_request("request-1", &json!({"state":"waiting"}), 100, 0)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store.active_pair_window_expiry("123456", 0).unwrap(),
        Some(100)
    );
    assert!(store.pair_window_active("123456", 0).unwrap());
    assert_eq!(
        store.active_pair_requests::<Value>(0).unwrap(),
        vec![json!({"state":"waiting"})]
    );
    assert_eq!(
        store.active_pair_request::<Value>("request-1", 0).unwrap(),
        Some(json!({"state":"waiting"}))
    );

    store
        .mutate("test", "approve", "approve", |change| {
            change.update_pair_request("request-1", &json!({"state":"approved"}))?;
            change.consume_pair_window_and_insert_device(
                "123456",
                "device-1",
                &json!({"deviceId":"device-1","revoked":false}),
                b"public-key",
                b"token-hash",
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store.active_pair_request::<Value>("request-1", 0).unwrap(),
        Some(json!({"state":"approved"}))
    );
    assert!(!store.pair_window_active("123456", 0).unwrap());
    let identity = store.device_identity(b"token-hash").unwrap().unwrap();
    assert_eq!(identity.device_id, "device-1");
    assert_eq!(identity.public_key, b"public-key");
    assert_eq!(
        store.device::<Value>("device-1").unwrap().unwrap()["revoked"],
        false
    );

    store
        .mutate("test", "revoke", "revoke", |change| {
            change.revoke_device("device-1", &json!({"deviceId":"device-1","revoked":true}))?;
            change.open_pair_window("fresh", 200, 101)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.device_identity(b"token-hash").unwrap().is_none());
    assert_eq!(
        store.device::<Value>("device-1").unwrap().unwrap()["revoked"],
        true
    );
    assert!(
        store
            .active_pair_request::<Value>("request-1", 101)
            .unwrap()
            .is_none()
    );
}

#[test]
fn agent_cleanup_operations_remove_only_the_selected_durable_rows() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed_workspace_and_pane(&mut store);
    let first = AttentionItem {
        attention_id: "attention-1".into(),
        pane_id: "p".into(),
        request_id: None,
        message: "First".into(),
        created_at: "fixed".into(),
    };
    let second = AttentionItem {
        attention_id: "attention-2".into(),
        message: "Second".into(),
        ..first.clone()
    };
    store
        .mutate("test", "agent-state", "agent-state", |change| {
            agents::save_runtime(change, "p", 1, &json!({}))?;
            agents::enqueue(
                change,
                &Outgoing {
                    operation_key: "operation".into(),
                    pane_id: "p".into(),
                    generation: 1,
                    instance_id: "instance".into(),
                    supervisor_id: "supervisor".into(),
                    bytes: b"{\"jsonrpc\":\"2.0\",\"method\":\"test\"}\n".to_vec(),
                    accepted_bytes: 0,
                    attempt: 0,
                },
            )?;
            agents::save_attention(change, "w", &first)?;
            agents::save_attention(change, "w", &second)?;
            for (method, result) in [
                ("session/load", ProbeResult::Unsupported),
                ("session/list", ProbeResult::Unauthenticated),
                ("session/set_config_option", ProbeResult::Inconclusive),
            ] {
                agents::save_probe(
                    change,
                    "fingerprint",
                    &ProbeCapability {
                        method: method.into(),
                        result,
                        checked_at: "fixed".into(),
                    },
                )?;
            }
            Ok((json!({}), vec![]))
        })
        .unwrap();

    store
        .mutate("test", "cleanup", "cleanup", |change| {
            change.retire_agent_outbox_process("p", "instance", "supervisor")?;
            change.remove_attention("p", "attention-1")?;
            change.clear_failed_probes("fingerprint")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(agents::outbox(&store, "p").unwrap().is_empty());
    assert_eq!(agents::attention(&store, "w").unwrap(), vec![second]);
    assert!(
        agents::probe(&store, "fingerprint", "session/load")
            .unwrap()
            .is_some()
    );
    assert!(
        agents::probe(&store, "fingerprint", "session/list")
            .unwrap()
            .is_none()
    );
    assert!(
        agents::probe(&store, "fingerprint", "session/set_config_option")
            .unwrap()
            .is_none()
    );
    store
        .mutate("test", "clear-attention", "clear-attention", |change| {
            change.clear_attention("p")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(agents::attention(&store, "w").unwrap().is_empty());
}
