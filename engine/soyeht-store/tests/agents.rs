#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rusqlite::{Connection, params};
use serde_json::{Value, json};
use soyeht_protocol::{
    AttachmentRef, Pane, PaneMode, PaneState, ProbeCapability, ProbeResult, PromptItem,
    PromptOrigin, PromptStatus, StateSource, TranscriptMessage, Workspace, limits,
};
use soyeht_store::{
    agents::{self, AgentModelRefusal, NativeRequest, Outgoing},
    database::{Error, SCHEMA_VERSION, Store},
    domain::PaneRecord,
};
use std::{fs, path::PathBuf};
use uuid::Uuid;

struct Profile(PathBuf);
impl Profile {
    fn new() -> Self {
        let p = Self(std::env::temp_dir().join(format!("sn-agent-store-{}", Uuid::new_v4())));
        fs::create_dir(&p.0).unwrap();
        p
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
fn seed(store: &mut Store) {
    store
        .mutate("test", "seed", "seed", |change| {
            change.insert_workspace(&Workspace {
                workspace_id: "w".into(),
                name: "Workspace".into(),
                path: "/tmp".into(),
                layout: json!({}),
            })?;
            change.insert_pane(&PaneRecord {
                pane: Pane {
                    pane_id: "p".into(),
                    workspace_id: "w".into(),
                    name: "Agent".into(),
                    cwd: "/tmp".into(),
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
                instance_key: "process".into(),
                instance_id: None,
                supervisor_id: None,
                sequence_base: 0,
            })?;
            agents::save_runtime(change, "p", 1, &json!({"nextOffset":0,"queue":[]}))?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}
fn incoming() -> NativeRequest {
    NativeRequest {
        request_id: "request-1".into(),
        pane_id: "p".into(),
        generation: 1,
        source_seq: u64::MAX - 20,
        native_id: Value::Null,
        method: "session/request_permission".into(),
        params: json!({"title":"Write file"}),
        public: json!({"requestId":"request-1","paneId":"p","expiresAt":"fixed"}),
        expires_ms: 1000,
        result: None,
    }
}
fn outgoing() -> Outgoing {
    Outgoing {
        operation_key: "reply-1".into(),
        pane_id: "p".into(),
        generation: 1,
        instance_id: "process-1".into(),
        supervisor_id: "supervisor-1".into(),
        accepted_bytes: 0,
        attempt: 0,
        bytes: b"{\"jsonrpc\":\"2.0\",\"id\":null,\"result\":{}}\n".to_vec(),
    }
}
fn message(seq: u64, text: &str, complete: bool) -> TranscriptMessage {
    serde_json::from_value(json!({"localId":format!("m-{seq}"),"generation":1,"seq":seq.to_string(),"role":"assistant","kind":"message","text":text,"complete":complete,"promptId":"prompt-1"})).unwrap()
}

#[test]
fn observed_model_refusals_are_bounded_upserted_and_survive_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    store
        .mutate("test", "refusals", "refusals", |tx| {
            for index in 0..=limits::MODEL_REFUSALS {
                let observed_at = chrono::DateTime::from_timestamp(1_700_000_000 + index as i64, 0)
                    .unwrap()
                    .to_rfc3339();
                agents::save_model_refusal(
                    tx,
                    &AgentModelRefusal {
                        harness_id: "test-agent".into(),
                        model_id: format!("model-{index:03}"),
                        reason: "Model is disabled".into(),
                        observed_at,
                    },
                )?;
            }
            agents::save_model_refusal(
                tx,
                &AgentModelRefusal {
                    harness_id: "test-agent".into(),
                    model_id: format!("model-{:03}", limits::MODEL_REFUSALS),
                    reason: "Subscription expired".into(),
                    observed_at: "2026-09-15T21:00:00Z".into(),
                },
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);

    let store = profile.store();
    let refusals = agents::model_refusals(&store, "test-agent").unwrap();
    assert_eq!(refusals.len(), limits::MODEL_REFUSALS);
    assert!(!refusals.iter().any(|item| item.model_id == "model-000"));
    assert_eq!(
        refusals
            .iter()
            .find(|item| item.model_id == format!("model-{:03}", limits::MODEL_REFUSALS))
            .unwrap()
            .reason,
        "Subscription expired"
    );
}

#[test]
fn observed_model_refusal_rejects_an_over_budget_reason() {
    let profile = Profile::new();
    let mut store = profile.store();
    let result = store.mutate("test", "refusal", "refusal", |tx| {
        agents::save_model_refusal(
            tx,
            &AgentModelRefusal {
                harness_id: "test-agent".into(),
                model_id: "model-a".into(),
                reason: "x".repeat(limits::MODEL_REFUSAL_REASON_BYTES + 1),
                observed_at: "2026-09-15T21:00:00Z".into(),
            },
        )?;
        Ok((json!({}), vec![]))
    });
    assert!(matches!(result, Err(Error::InvalidAgentModelRefusal)));
}

#[test]
fn migration_from_v21_preserves_existing_state_and_adds_model_refusals() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    drop(store);

    let connection = Connection::open(profile.0.join("soyeht.db")).unwrap();
    connection
        .execute_batch("DROP TABLE agent_model_refusals; PRAGMA user_version=21;")
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    assert_eq!(
        store.snapshot().unwrap().sections[0].1[0]["workspaceId"],
        "w"
    );
    assert!(
        agents::model_refusals(&store, "test-agent")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn native_replay_is_private_until_atomic_publication_and_survives_restart() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    store
        .mutate("test", "original", "original", |tx| {
            agents::save_message(tx, "p", &message(1, "Original", true))?;
            agents::save_runtime(tx, "p", 2, &json!({"phase":"Loading"}))?;
            let mut staged = message(2, "Restored", true);
            staged.generation = 2;
            agents::stage_message(tx, "p", &staged)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);
    let mut store = profile.store();
    assert_eq!(
        agents::transcript(&store, "p", None, 100).unwrap().items[0].text,
        "Original"
    );
    let failed = store.mutate("test", "publish", "publish", |tx| {
        agents::publish_replay(tx, "p", 2)?;
        Err(Error::TestFixture("crash before publication commit"))
    });
    assert!(failed.is_err());
    assert_eq!(
        agents::transcript(&store, "p", None, 100).unwrap().items[0].text,
        "Original"
    );
    store
        .mutate("test", "publish", "publish", |tx| {
            agents::publish_replay(tx, "p", 2)?;
            Ok((
                json!({}),
                vec![(
                    "pane.session_started".into(),
                    json!({"paneId":"p","generation":2}),
                )],
            ))
        })
        .unwrap();
    let page = agents::transcript(&store, "p", None, 100).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].text, "Restored");
    assert_eq!(page.items[0].generation, 2);
    let staged: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_replay_transcript", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(staged, 0);
}

#[test]
fn request_identity_deadline_answer_and_native_outbox_survive_crash_boundaries() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    store
        .mutate("native", "received", "frame", |tx| {
            agents::receive(tx, &incoming())?;
            agents::save_runtime(
                tx,
                "p",
                1,
                &json!({"nextOffset":u64::MAX.to_string(),"pendingCall":"s-7"}),
            )?;
            Ok((
                json!({}),
                vec![("pane.permission_requested".into(), incoming().public)],
            ))
        })
        .unwrap();
    drop(store);
    let mut store = profile.store();
    let runtime: Value = agents::runtime(&store, "p").unwrap().unwrap();
    assert_eq!(runtime["pendingCall"], "s-7");
    let mut replay = incoming();
    replay.request_id = "new-id-must-not-win".into();
    replay.expires_ms = 9000;
    store
        .mutate("native", "replayed", "frame", |tx| {
            assert_eq!(agents::receive(tx, &replay)?, incoming());
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .sections
            .iter()
            .find(|s| s.0 == "requests")
            .unwrap()
            .1,
        json!([{"kind":"permission","request":incoming().public}])
    );
    let failed = store.mutate("local", "answer", "answer", |tx| {
        agents::answer(tx, "request-1", &json!({"by":"mac","result":"selected"}))?;
        agents::enqueue(tx, &outgoing())?;
        Err(Error::TestFixture("crash before commit"))
    });
    assert!(failed.is_err());
    assert!(
        agents::request(&store, "request-1")
            .unwrap()
            .unwrap()
            .result
            .is_none()
    );
    assert!(agents::outbox(&store, "p").unwrap().is_empty());
    let winning = json!({"by":"iphone","result":"selected"});
    store
        .mutate("phone", "answer", "answer", |tx| {
            assert!(agents::answer(tx, "request-1", &winning)?.first);
            agents::enqueue(tx, &outgoing())?;
            Ok((
                winning.clone(),
                vec![("pane.permission_answered".into(), winning.clone())],
            ))
        })
        .unwrap();
    drop(store);
    let mut store = profile.store();
    assert_eq!(agents::outbox(&store, "p").unwrap(), vec![outgoing()]);
    store
        .mutate("mac", "late", "answer", |tx| {
            let decision =
                agents::answer(tx, "request-1", &json!({"by":"mac","result":"cancelled"}))?;
            assert!(!decision.first);
            assert_eq!(decision.result, winning);
            Ok((decision.result, vec![]))
        })
        .unwrap();
    assert!(agents::open_requests(&store, "p").unwrap().is_empty());
    store
        .mutate("native", "delivered", "receipt", |tx| {
            let sent = agents::advance(tx, &outgoing(), outgoing().bytes.len())?;
            agents::delivered(tx, &sent)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);
    let store = profile.store();
    assert!(agents::outbox(&store, "p").unwrap().is_empty());
    assert_eq!(
        agents::request(&store, "request-1")
            .unwrap()
            .unwrap()
            .result,
        Some(winning)
    );
}

#[test]
fn full_prompt_queue_retains_escaped_text_and_changes_atomically_with_the_checkpoint() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    store.mutate("test","queue","queue",|tx| {
        for n in 0..64 {
            let item=serde_json::from_value(json!({"promptId":format!("prompt-{n}"),"text":"\0".repeat(limits::PROMPT_BYTES),"origin":"human","status":"queued","queuedAt":"fixed"}))?;
            agents::enqueue_prompt(tx,"p",&item)?;
        }
        agents::save_runtime(tx,"p",1,&json!({"queueRevision":64}))?;
        Ok((json!({}),vec![]))
    }).unwrap();
    drop(store);
    let mut store = profile.store();
    let items = agents::queue(&store, "p").unwrap();
    assert_eq!(items.len(), 64);
    for (n, item) in items.iter().enumerate() {
        assert_eq!(item.prompt_id, format!("prompt-{n}"));
        assert_eq!(item.text, "\0".repeat(limits::PROMPT_BYTES));
    }
    let mut overflow = items[0].clone();
    overflow.prompt_id = "overflow".into();
    assert!(
        store
            .mutate("test", "overflow", "overflow", |tx| {
                agents::enqueue_prompt(tx, "p", &overflow)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    assert!(
        store
            .mutate("test", "failed-take", "failed-take", |tx| {
                agents::remove_prompt(tx, "p", "prompt-0")?;
                agents::save_runtime(tx, "p", 1, &json!({"queueRevision":65,"active":"prompt-0"}))?;
                Err(Error::TestFixture("injected crash before queue commit"))
            })
            .is_err()
    );
    assert_eq!(agents::queue(&store, "p").unwrap().len(), 64);
    assert_eq!(
        agents::runtime::<Value>(&store, "p").unwrap().unwrap()["queueRevision"],
        64
    );
}

fn prompt_with_image() -> (PromptItem, agents::PromptAttachment) {
    let reference = AttachmentRef {
        attachment_id: "00112233-4455-4677-8899-aabbccddeeff".into(),
        mime_type: "image/png".into(),
        name: "element.png".into(),
        bytes: "4".into(),
    };
    (
        PromptItem {
            prompt_id: "prompt-image".into(),
            text: "Inspect this element".into(),
            origin: PromptOrigin::Human,
            status: PromptStatus::Queued,
            queued_at: "fixed".into(),
            generation: None,
            attachments: Some(vec![reference.clone()]),
        },
        agents::PromptAttachment {
            reference,
            bytes: vec![137, 80, 78, 71],
        },
    )
}

#[test]
fn prompt_images_commit_with_the_queue_survive_reopen_and_cascade_on_remove() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let (prompt, attachment) = prompt_with_image();
    store
        .mutate("test", "image", "image", |tx| {
            agents::enqueue_prompt_with_attachments(
                tx,
                "p",
                &prompt,
                std::slice::from_ref(&attachment),
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);

    let mut store = profile.store();
    assert_eq!(agents::queue(&store, "p").unwrap(), vec![prompt.clone()]);
    assert_eq!(
        agents::prompt_attachments(&store, &prompt.prompt_id).unwrap(),
        vec![attachment.clone()]
    );
    store
        .mutate("test", "remove-image", "remove-image", |tx| {
            assert!(agents::remove_prompt(tx, "p", &prompt.prompt_id)?);
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(
        agents::prompt_attachments(&store, &prompt.prompt_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn prompt_images_fail_closed_on_metadata_bytes_and_idempotency_mismatch() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let (prompt, attachment) = prompt_with_image();
    let mut wrong = attachment.clone();
    wrong.bytes.push(0);
    assert!(
        store
            .mutate("test", "wrong", "wrong", |tx| {
                agents::enqueue_prompt_with_attachments(tx, "p", &prompt, &[wrong])?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    store
        .mutate("test", "right", "right", |tx| {
            agents::enqueue_prompt_with_attachments(
                tx,
                "p",
                &prompt,
                std::slice::from_ref(&attachment),
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let mut changed = attachment;
    changed.bytes[3] = 70;
    assert!(
        store
            .mutate("test", "changed", "changed", |tx| {
                agents::enqueue_prompt_with_attachments(tx, "p", &prompt, &[changed])?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
}

#[test]
fn migration_from_populated_v18_preserves_the_queue_and_adds_prompt_images() {
    let profile = Profile::new();
    let connection = Connection::open(profile.0.join("soyeht.db")).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON").unwrap();
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
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            "INSERT INTO metadata(key,value) VALUES ('db_id','old'),('engine_generation','1');
             INSERT INTO workspaces(id,payload) VALUES ('w','{}');
             INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES ('p','w','{}','process');
             INSERT INTO agent_runtime(pane_id,generation,payload) VALUES ('p',1,'{}');
             INSERT INTO agent_prompt_queue(prompt_id,pane_id,payload) VALUES
               ('old-prompt','p','{\"promptId\":\"old-prompt\",\"text\":\"keep\",\"origin\":\"human\",\"status\":\"queued\",\"queuedAt\":\"fixed\"}');",
        )
        .unwrap();
    drop(connection);

    let mut store = profile.store();
    assert_eq!(agents::queue(&store, "p").unwrap()[0].text, "keep");
    assert_eq!(
        store
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    let (prompt, attachment) = prompt_with_image();
    store
        .mutate("test", "new-image", "new-image", |tx| {
            agents::enqueue_prompt_with_attachments(tx, "p", &prompt, &[attachment])?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

#[test]
fn partial_and_zero_byte_receipts_keep_progress_without_repeating_prior_bytes() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    store
        .mutate("test", "enqueue", "enqueue", |tx| {
            agents::enqueue(tx, &outgoing())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    store
        .mutate("test", "partial", "partial", |tx| {
            let next = agents::advance(tx, &outgoing(), 11)?;
            assert_eq!(next.accepted_bytes, 11);
            assert_eq!(next.attempt, 1);
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);
    let mut store = profile.store();
    let prior = agents::outbox(&store, "p").unwrap().remove(0);
    assert_eq!(prior.accepted_bytes, 11);
    assert!(
        store
            .mutate("test", "duplicate", "duplicate", |tx| {
                agents::advance(tx, &outgoing(), 11)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    assert!(
        store
            .mutate("test", "incomplete", "incomplete", |tx| {
                agents::delivered(tx, &prior)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    store
        .mutate("test", "zero", "zero", |tx| {
            let next = agents::advance(tx, &prior, 0)?;
            assert_eq!(next.accepted_bytes, 11);
            assert_eq!(next.attempt, 2);
            agents::enqueue(tx, &outgoing())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let stored = agents::outgoing(&store, "reply-1").unwrap().unwrap();
    assert_eq!(stored.accepted_bytes, 11);
    assert_eq!(stored.attempt, 2);
}

#[test]
fn process_identity_stale_generation_and_different_native_replays_are_refused() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    store
        .mutate("test", "initial", "initial", |tx| {
            agents::receive(tx, &incoming())?;
            agents::enqueue(tx, &outgoing())?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let mut wrong = outgoing();
    wrong.instance_id = "replacement".into();
    assert!(
        store
            .mutate("test", "wrong", "wrong", |tx| {
                agents::delivered(tx, &wrong)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    assert!(
        store
            .mutate("test", "wrong-enqueue", "wrong", |tx| {
                agents::enqueue(tx, &wrong)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    let mut replay = incoming();
    replay.native_id = json!(0);
    assert!(
        store
            .mutate("test", "wrong-id", "wrong", |tx| {
                agents::receive(tx, &replay)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    store
        .mutate("test", "replacement", "replacement", |tx| {
            agents::save_runtime(tx, "p", 2, &json!({}))?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    for kind in [0, 1, 2] {
        assert!(
            store
                .mutate("test", &format!("stale-{kind}"), "stale", |tx| {
                    match kind {
                        0 => agents::save_runtime(tx, "p", 1, &json!({}))?,
                        1 => agents::enqueue(tx, &outgoing())?,
                        _ => {
                            agents::answer(tx, "request-1", &json!({}))?;
                        }
                    }
                    Ok((json!({}), vec![]))
                })
                .is_err()
        );
    }
    assert_eq!(
        agents::outbox(&store, "p").unwrap()[0].instance_id,
        "process-1"
    );
}

#[test]
fn transcript_pages_are_bounded_ordered_and_updates_cannot_change_identity() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let start = i64::MAX as u64;
    for seq in start..start + 5 {
        store
            .mutate("test", &seq.to_string(), "message", |tx| {
                agents::save_message(tx, "p", &message(seq, &"🌍".repeat(40000), false))?;
                Ok((json!({}), vec![]))
            })
            .unwrap();
    }
    let page = agents::transcript(&store, "p", None, 200).unwrap();
    assert_eq!(page.items.len(), 3);
    assert!(serde_json::to_vec(&page).unwrap().len() <= limits::TRANSCRIPT_PAGE_BYTES);
    assert_eq!(page.items[0].seq, (start + 2).to_string());
    let earlier = agents::transcript(
        &store,
        "p",
        Some(page.next_cursor.unwrap().parse().unwrap()),
        200,
    )
    .unwrap();
    assert_eq!(earlier.items.len(), 2);
    assert_eq!(earlier.items[0].seq, start.to_string());
    assert!(earlier.next_cursor.is_none());
    let finished = message(start, "Complete", true);
    store
        .mutate("test", "finish", "finish", |tx| {
            agents::save_message(tx, "p", &finished)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(
        store
            .mutate("test", "rewrite", "rewrite", |tx| {
                agents::save_message(tx, "p", &message(start, "Changed", true))?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    let mut changed = message(start + 1, "Wrong identity", false);
    changed.local_id = "different".into();
    assert!(
        store
            .mutate("test", "identity", "identity", |tx| {
                agents::save_message(tx, "p", &changed)?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
    assert!(
        store
            .mutate("test", "oversize", "oversize", |tx| {
                agents::save_message(
                    tx,
                    "p",
                    &message(
                        start + 9,
                        &"x".repeat(limits::TRANSCRIPT_MESSAGE_BYTES),
                        true,
                    ),
                )?;
                Ok((json!({}), vec![]))
            })
            .is_err()
    );
}

#[test]
fn transcript_retention_discards_whole_old_messages_and_probe_cache_tracks_fingerprint() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    store
        .mutate("test", "history", "history", |tx| {
            for seq in 0..145 {
                agents::save_message(tx, "p", &message(seq, &"x".repeat(240000), true))?;
            }
            agents::save_probe(
                tx,
                "fingerprint-1",
                &ProbeCapability {
                    method: "session/load".into(),
                    result: ProbeResult::Unsupported,
                    checked_at: "fixed".into(),
                },
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);
    let store = profile.store();
    let (first, total): (Vec<u8>, i64) = store
        .connection()
        .query_row(
            "SELECT MIN(seq),SUM(bytes) FROM agent_transcript",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(u64::from_be_bytes(first.try_into().unwrap()) > 0);
    assert!(total <= limits::PANE_RETENTION_BYTES as i64);
    assert_eq!(
        agents::probe(&store, "fingerprint-1", "session/load")
            .unwrap()
            .unwrap()
            .result,
        ProbeResult::Unsupported
    );
    assert!(
        agents::probe(&store, "fingerprint-2", "session/load")
            .unwrap()
            .is_none()
    );
}

#[test]
fn migration_from_v3_preserves_live_panes_and_takes_a_backup() {
    let profile = Profile::new();
    let old = Connection::open(profile.0.join("soyeht.db")).unwrap();
    for sql in [
        include_str!("../migrations/001-initial.sql"),
        include_str!("../migrations/002-client-state.sql"),
        include_str!("../migrations/003-english-values.sql"),
    ] {
        old.execute_batch(sql).unwrap();
    }
    old.execute(
        "INSERT INTO metadata VALUES ('db_id','stable'),('engine_generation','23')",
        [],
    )
    .unwrap();
    old.execute("INSERT INTO workspaces VALUES ('w','{}',0)", [])
        .unwrap();
    let pane = json!({"paneId":"p","state":"idle","pid":123,"generation":2});
    old.execute("INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES ('p','w',?1,'same-process')",params![pane.to_string()]).unwrap();
    drop(old);
    let mut store = profile.store();
    assert_eq!(store.db_id, "stable");
    assert_eq!(store.next_engine_generation().unwrap(), 24);
    assert_eq!(store.snapshot().unwrap().sections[1].1, json!([pane]));
    let files = fs::read_dir(profile.0.join("backups"))
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let backup = Connection::open(files[0].path()).unwrap();
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
}

fn populate_v4(profile: &Profile) -> (Value, Outgoing) {
    let mut old = Connection::open(profile.0.join("soyeht.db")).unwrap();
    for sql in [
        include_str!("../migrations/001-initial.sql"),
        include_str!("../migrations/002-client-state.sql"),
        include_str!("../migrations/003-english-values.sql"),
        include_str!("../migrations/004-agent-sessions.sql"),
    ] {
        old.execute_batch(sql).unwrap();
    }
    old.execute(
        "INSERT INTO metadata VALUES ('db_id','v4-live'),('engine_generation','8')",
        [],
    )
    .unwrap();
    old.execute("INSERT INTO workspaces VALUES ('w','{}',0)", [])
        .unwrap();
    old.execute("INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES ('p','w','{}','same-process')", []).unwrap();
    let checkpoint =
        json!({"phase":"Ready","acknowledged_offset":101,"session_id":"native-session"});
    let request = incoming();
    let output = outgoing();
    let transcript = message(1, "Durable history", true);
    let transcript_json = serde_json::to_string(&transcript).unwrap();
    let tx = old.transaction().unwrap();
    tx.execute(
        "INSERT INTO agent_runtime(pane_id,generation,payload) VALUES (?1,?2,?3)",
        params!["p", 1, checkpoint.to_string()],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO agent_requests(request_id,pane_id,generation,source_seq,native_id,method,params,public,expires_ms,result) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",
        params![
            request.request_id,
            request.pane_id,
            request.generation,
            request.source_seq.to_be_bytes().as_slice(),
            request.native_id.to_string(),
            request.method,
            request.params.to_string(),
            request.public.to_string(),
            request.expires_ms,
        ],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO agent_outbox(operation_key,pane_id,generation,instance_id,supervisor_id,bytes,accepted_bytes,attempt) VALUES (?1,?2,?3,?4,?5,?6,7,2)",
        params![
            output.operation_key,
            output.pane_id,
            output.generation,
            output.instance_id,
            output.supervisor_id,
            output.bytes,
        ],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO agent_transcript(pane_id,seq,local_id,generation,payload,bytes) VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            "p",
            1_u64.to_be_bytes().as_slice(),
            transcript.local_id,
            transcript.generation,
            transcript_json,
            transcript_json.len() as i64,
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(old);
    (checkpoint, output)
}

#[test]
fn migration_from_populated_v4_preserves_native_receipts_and_offsets() {
    let profile = Profile::new();
    let (checkpoint, output) = populate_v4(&profile);
    let mut store = profile.store();
    assert_eq!(store.db_id, "v4-live");
    assert_eq!(store.next_engine_generation().unwrap(), 9);
    assert_eq!(
        agents::runtime::<Value>(&store, "p").unwrap().unwrap(),
        checkpoint
    );
    assert_eq!(
        agents::open_requests(&store, "p").unwrap()[0].source_seq,
        incoming().source_seq
    );
    let resumed = agents::outbox(&store, "p").unwrap();
    assert_eq!(resumed[0].bytes, output.bytes);
    assert_eq!(resumed[0].accepted_bytes, 7);
    assert_eq!(resumed[0].attempt, 2);
    assert_eq!(
        agents::transcript(&store, "p", None, 100).unwrap().items[0].text,
        "Durable history"
    );
    assert!(agents::attention(&store, "w").unwrap().is_empty());
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
    let backups = fs::read_dir(profile.0.join("backups"))
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(backups.len(), 1);
    let backup = Connection::open(backups[0].path()).unwrap();
    assert_eq!(
        backup
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        backup
            .query_row(
                "SELECT accepted_bytes FROM agent_outbox WHERE pane_id='p'",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap(),
        7
    );
}

#[test]
fn migration_from_v5_adds_reconnectable_authentication_state() {
    let profile = Profile::new();
    let old = Connection::open(profile.0.join("soyeht.db")).unwrap();
    for sql in [
        include_str!("../migrations/001-initial.sql"),
        include_str!("../migrations/002-client-state.sql"),
        include_str!("../migrations/003-english-values.sql"),
        include_str!("../migrations/004-agent-sessions.sql"),
        include_str!("../migrations/005-native-replay.sql"),
    ] {
        old.execute_batch(sql).unwrap();
    }
    old.execute(
        "INSERT INTO metadata VALUES ('db_id','v5-live'),('engine_generation','8')",
        [],
    )
    .unwrap();
    old.execute("INSERT INTO workspaces VALUES ('w','{}',0)", [])
        .unwrap();
    let pane = json!({
        "paneId": "p", "workspaceId": "w", "name": "Agent", "cwd": "/tmp",
        "mode": "agent", "state": "starting", "source": "protocol", "confidence": 1,
        "phrase": "Sign-in required", "generation": 1, "cols": 80, "rows": 24,
        "record": true,
        "agent": {"harness": "auth", "version": "1", "fingerprint": "fp",
            "options": [], "permission": "ask", "yolo": false, "queue": [], "draft": false}
    });
    old.execute(
        "INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES ('p','w',?1,'same-process')",
        [pane.to_string()],
    )
    .unwrap();
    drop(old);

    let mut store = profile.store();
    let panes = store
        .snapshot()
        .unwrap()
        .sections
        .into_iter()
        .find(|(section, _)| section == "panes")
        .unwrap()
        .1;
    assert_eq!(panes[0]["agent"]["authMethods"], json!([]));
    assert_eq!(panes[0]["agent"]["role"], "worker");
    let _: soyeht_protocol::Pane = serde_json::from_value(panes[0].clone()).unwrap();
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        soyeht_store::database::SCHEMA_VERSION
    );
}
