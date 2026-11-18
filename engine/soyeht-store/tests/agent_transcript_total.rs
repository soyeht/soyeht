#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use rusqlite::{
    Connection, StatementStatus, params,
    trace::{TraceEvent, TraceEventCodes},
};
use serde_json::json;
use soyeht_protocol::{PaneMode, TranscriptMessage, Workspace, limits};
use soyeht_store::{agents, database::Store, domain::PaneRecord};
use std::cell::Cell;
use support::{Profile, commit};

fn seed(store: &mut Store) {
    commit(store, "seed", |change| {
        change.insert_workspace(&Workspace {
            workspace_id: "w".into(),
            name: "Workspace".into(),
            path: "/tmp".into(),
            layout: json!({}),
        })?;
        for id in ["a", "b"] {
            change.insert_pane(&PaneRecord {
                pane: support::pane(id, "w", PaneMode::Agent, "/tmp"),
                closed: false,
                instance_key: format!("process-{id}"),
                instance_id: None,
                supervisor_id: None,
                sequence_base: 0,
            })?;
            agents::save_runtime(change, id, 1, &json!({"nextOffset":0,"queue":[]}))?;
        }
        Ok(())
    });
}

/// A streaming message, so the same sequence can be saved again at another size.
fn message(pane: &str, seq: u64, size: usize) -> TranscriptMessage {
    serde_json::from_value(json!({"localId":format!("{pane}-{seq}"),"generation":1,"seq":seq.to_string(),"role":"assistant","kind":"message","text":"x".repeat(size),"complete":false,"promptId":"prompt-1"})).unwrap()
}

/// How many (pane, table) pairs have a total that differs from the sum of their rows; a total
/// kept for no rows counts too.
fn wrong_totals(db: &Connection) -> i64 {
    db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM (
           SELECT pane_id, 0 AS staged, SUM(bytes) AS bytes FROM agent_transcript GROUP BY pane_id
           UNION ALL SELECT pane_id, 1, SUM(bytes) FROM agent_replay_transcript GROUP BY pane_id
           UNION ALL SELECT pane_id, staged, -bytes FROM agent_transcript_total
         ) GROUP BY pane_id, staged HAVING SUM(bytes) <> 0)",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn the_total_of_a_pane_equals_the_sum_of_its_rows_after_any_writes_and_retention_trims() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    // Fill pane "a" past the retention limit, so the oldest messages are trimmed as it grows.
    for batch in 0..4 {
        commit(&mut store, &format!("fill-{batch}"), |change| {
            for seq in batch * 50..(batch + 1) * 50 {
                agents::save_message(change, "a", &message("a", seq, 200_000))?;
            }
            Ok(())
        });
    }
    let (rows, total): (i64, i64) = store
        .connection()
        .query_row(
            "SELECT COUNT(*), SUM(bytes) FROM agent_transcript WHERE pane_id='a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(
        rows < 200 && total <= limits::PANE_RETENTION_BYTES as i64,
        "retention trimmed"
    );
    assert_eq!(wrong_totals(store.connection()), 0);
    // Then a random mix of saves (new and rewritten), staging, publishing and discarding a
    // replay, and writes the store's own functions never make.
    for step in 0..400 {
        let pane = ["a", "b"][usize::from(next(4) == 0)];
        let (seq, large) = (150 + next(300), next(3) == 0);
        let size = 50 + next(if large { 250_000 } else { 2_000 }) as usize;
        let key = format!("step-{step}");
        let message = message(pane, seq, size);
        let raw = |sql: &str| {
            let at = seq.to_be_bytes();
            store
                .connection()
                .execute(sql, params![pane, at.as_slice()])
                .unwrap();
        };
        match next(20) {
            0..=8 => commit(&mut store, &key, |c| {
                agents::save_message(c, pane, &message)
            }),
            9..=13 => commit(&mut store, &key, |c| {
                agents::stage_message(c, pane, &message)
            }),
            14 => commit(&mut store, &key, |c| agents::publish_replay(c, pane, 1)),
            15 => commit(&mut store, &key, |c| agents::discard_replay(c, pane)),
            16..=17 => raw("DELETE FROM agent_transcript WHERE pane_id=?1 AND seq=?2"),
            _ => {
                raw("UPDATE agent_replay_transcript SET bytes=bytes+1 WHERE pane_id=?1 AND seq<>?2")
            }
        }
        assert_eq!(wrong_totals(store.connection()), 0, "after step {step}");
    }
}

thread_local! {
    static STEPS: Cell<i64> = const { Cell::new(0) };
}

/// The virtual machine steps SQLite takes to run `work`: a count that does not depend on the
/// clock or on the machine.
fn steps(store: &mut Store, work: impl FnOnce(&mut Store)) -> i64 {
    store.connection().trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(|event| {
            if let TraceEvent::Profile(statement, _) = event {
                let taken = statement.get_status(StatementStatus::VmStep);
                STEPS.with(|steps| steps.set(steps.get() + i64::from(taken)));
            }
        }),
    );
    STEPS.with(|steps| steps.set(0));
    work(store);
    store.connection().trace_v2(TraceEventCodes::empty(), None);
    STEPS.with(Cell::get)
}

#[test]
fn saving_a_message_takes_the_same_work_whatever_the_length_of_the_conversation() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    commit(&mut store, "history", |change| {
        for seq in 0..20 {
            agents::save_message(change, "a", &message("a", seq, 1_000))?;
        }
        for seq in 0..2_000 {
            agents::save_message(change, "b", &message("b", seq, 1_000))?;
        }
        Ok(())
    });
    let mut save = |pane: &str| {
        steps(&mut store, |store| {
            commit(store, &format!("save-{pane}"), |change| {
                agents::save_message(change, pane, &message(pane, 5_000, 1_000))
            })
        })
    };
    let (short, long) = (save("a"), save("b"));
    assert!(
        long <= short + short / 10,
        "{short} steps with 20 messages, {long} with 2,000"
    );
    // The measure sees the cost of history: the sum the store used to take is far larger.
    let sum = steps(&mut store, |store| {
        let sql = "SELECT SUM(bytes) FROM agent_transcript WHERE pane_id='b'";
        store
            .connection()
            .query_row(sql, [], |row| row.get::<_, i64>(0))
            .unwrap();
    });
    assert!(
        sum > 20 * long,
        "the old sum took {sum} steps against {long}"
    );
}

#[test]
fn a_database_of_version_38_gets_the_totals_of_the_messages_it_already_holds() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    commit(&mut store, "history", |change| {
        for seq in 0..3 {
            agents::save_message(change, "a", &message("a", seq, 1_000))?;
            agents::stage_message(change, "b", &message("b", seq, 500))?;
        }
        Ok(())
    });
    drop(store);
    profile
        .connection()
        .execute_batch("DROP TABLE agent_transcript_total; PRAGMA user_version = 38;")
        .unwrap();
    let store = profile.store();
    assert_eq!(wrong_totals(store.connection()), 0);
    let kept: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_transcript_total", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, 2);
}
