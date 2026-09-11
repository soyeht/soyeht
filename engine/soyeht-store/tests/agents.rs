use rusqlite::{Connection, params};
use serde_json::{Value, json};
use soyeht_protocol::{ProbeCapability, ProbeResult, TranscriptMessage, limits};
use soyeht_store::{
    agents::{self, NativeRequest, Outgoing},
    database::{Error, Store},
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
    store.mutate("test","seed","seed",|tx| {
        tx.execute("INSERT INTO workspaces(id,payload) VALUES ('w','{}')",[])?;
        tx.execute("INSERT INTO panes(id,workspace_id,payload,instance_key) VALUES ('p','w','{}','process')",[])?;
        agents::save_runtime(tx,"p",1,&json!({"nextOffset":0,"queue":[]}))?;
        Ok((json!({}),vec![]))
    }).unwrap();
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
        agents::transcript(store.connection(), "p", None, 100)
            .unwrap()
            .items[0]
            .text,
        "Original"
    );
    let failed = store.mutate("test", "publish", "publish", |tx| {
        agents::publish_replay(tx, "p", 2)?;
        Err(Error::Invalid("crash before publication commit"))
    });
    assert!(failed.is_err());
    assert_eq!(
        agents::transcript(store.connection(), "p", None, 100)
            .unwrap()
            .items[0]
            .text,
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
    let page = agents::transcript(store.connection(), "p", None, 100).unwrap();
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
    let runtime: Value = agents::runtime(store.connection(), "p").unwrap().unwrap();
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
        Err(Error::Invalid("crash before commit"))
    });
    assert!(failed.is_err());
    assert!(
        agents::request(store.connection(), "request-1")
            .unwrap()
            .unwrap()
            .result
            .is_none()
    );
    assert!(agents::outbox(store.connection(), "p").unwrap().is_empty());
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
    assert_eq!(
        agents::outbox(store.connection(), "p").unwrap(),
        vec![outgoing()]
    );
    store
        .mutate("mac", "late", "answer", |tx| {
            let decision =
                agents::answer(tx, "request-1", &json!({"by":"mac","result":"cancelled"}))?;
            assert!(!decision.first);
            assert_eq!(decision.result, winning);
            Ok((decision.result, vec![]))
        })
        .unwrap();
    assert!(
        agents::open_requests(store.connection(), "p")
            .unwrap()
            .is_empty()
    );
    store
        .mutate("native", "delivered", "receipt", |tx| {
            let sent = agents::advance(tx, &outgoing(), outgoing().bytes.len())?;
            agents::delivered(tx, &sent)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);
    let store = profile.store();
    assert!(agents::outbox(store.connection(), "p").unwrap().is_empty());
    assert_eq!(
        agents::request(store.connection(), "request-1")
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
    let items = agents::queue(store.connection(), "p").unwrap();
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
                Err(Error::Invalid("injected crash before queue commit"))
            })
            .is_err()
    );
    assert_eq!(agents::queue(store.connection(), "p").unwrap().len(), 64);
    assert_eq!(
        agents::runtime::<Value>(store.connection(), "p")
            .unwrap()
            .unwrap()["queueRevision"],
        64
    );
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
    let prior = agents::outbox(store.connection(), "p").unwrap().remove(0);
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
            assert_eq!(agents::outgoing(tx, "reply-1")?, Some(next));
            Ok((json!({}), vec![]))
        })
        .unwrap();
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
        agents::outbox(store.connection(), "p").unwrap()[0].instance_id,
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
    let page = agents::transcript(store.connection(), "p", None, 200).unwrap();
    assert_eq!(page.items.len(), 3);
    assert!(serde_json::to_vec(&page).unwrap().len() <= limits::TRANSCRIPT_PAGE_BYTES);
    assert_eq!(page.items[0].seq, (start + 2).to_string());
    let earlier = agents::transcript(
        store.connection(),
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
        agents::probe(store.connection(), "fingerprint-1", "session/load")
            .unwrap()
            .unwrap()
            .result,
        ProbeResult::Unsupported
    );
    assert!(
        agents::probe(store.connection(), "fingerprint-2", "session/load")
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

#[test]
fn migration_from_populated_v4_preserves_native_receipts_and_offsets() {
    let profile = Profile::new();
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
    let tx = old.transaction().unwrap();
    agents::save_runtime(&tx, "p", 1, &checkpoint).unwrap();
    agents::receive(&tx, &incoming()).unwrap();
    let output = outgoing();
    agents::enqueue(&tx, &output).unwrap();
    let progress = agents::advance(&tx, &output, 3).unwrap();
    agents::advance(&tx, &progress, 4).unwrap();
    agents::save_message(&tx, "p", &message(1, "Durable history", true)).unwrap();
    tx.commit().unwrap();
    drop(old);
    let mut store = profile.store();
    assert_eq!(store.db_id, "v4-live");
    assert_eq!(store.next_engine_generation().unwrap(), 9);
    assert_eq!(
        agents::runtime::<Value>(store.connection(), "p")
            .unwrap()
            .unwrap(),
        checkpoint
    );
    assert_eq!(
        agents::open_requests(store.connection(), "p").unwrap()[0].source_seq,
        incoming().source_seq
    );
    let resumed = agents::outbox(store.connection(), "p").unwrap();
    assert_eq!(resumed[0].bytes, output.bytes);
    assert_eq!(resumed[0].accepted_bytes, 7);
    assert_eq!(resumed[0].attempt, 2);
    assert_eq!(
        agents::transcript(store.connection(), "p", None, 100)
            .unwrap()
            .items[0]
            .text,
        "Durable history"
    );
    assert!(
        agents::attention(store.connection(), "w")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .connection()
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        5
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
    assert_eq!(agents::outbox(&backup, "p").unwrap()[0].accepted_bytes, 7);
}
