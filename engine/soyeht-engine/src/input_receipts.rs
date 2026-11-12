//! The outcome of a terminal input is made durable with the next write of inputs, not alone
//! (ADR 0147, step R).
//!
//! A `pane.input` records its intent before the irreversible write to the supervisor and its
//! outcome after it: two commits, and a commit is a flush of the disk under the Core lock. The
//! outcome only lets a retry with the same key be answered from the database. Until it is
//! recorded, the intent is still there: a retry finds it, sends the same operation key, and the
//! supervisor, which keeps the operation until the engine confirms it, answers once. So the reply
//! may go before the outcome is durable, and the outcome waits for the next write of an input
//! (the commit of its intent carries it) or for a quiet moment ([`LINGER_MS`], or the engine
//! stopping, which records it and sends the confirmation), and the supervisor
//! is told to forget the operation only after that.
//!
//! Nothing weaker is promised than before: the intent is durable before the write to the terminal,
//! as it was, and the confirmation of an operation still follows the commit of its outcome.

use crate::tick_due::Next;
use crate::{
    core::Core,
    errors::{self, Result},
};
use soyeht_ptyd::wire::Request;
use soyeht_store::domain::FinishedInput;

/// How long an outcome waits for the next input before it is recorded alone. A person who is
/// typing pauses for less than this between keys, so the keys share their commits: one commit a
/// key, not two (ADR 0147, step R). A quarter second left a key typed after a short pause to be
/// recorded by itself. An outcome that waits is answered to a retry from memory, its intent is
/// durable, and the engine records it when it stops, so the only cost of waiting is that a crash
/// leaves it to the orphan sweep ([`ORPHAN_AFTER_S`]), which it already did.
pub(crate) const LINGER_MS: i64 = 3_000;

/// How long after it was recorded an intent whose operation the supervisor holds is left for its
/// client to retry before it is forgotten: a client retries within seconds.
const ORPHAN_AFTER_S: i64 = 60;
/// How long the sweep leaves an intent that is not old enough yet alone.
const ORPHAN_RECHECK_MS: i64 = 10_000;

/// An accepted input whose outcome is not durable yet.
pub(crate) struct Waiting {
    finished: FinishedInput,
    operation_key: String,
    queued_ms: i64,
}

impl Waiting {
    pub(crate) fn finished(&self) -> &FinishedInput {
        &self.finished
    }

    /// An outcome of the input `key` of `client`, queued at `queued_ms`.
    #[cfg(test)]
    pub(crate) fn for_test(key: &str, operation_key: &str, queued_ms: i64) -> Self {
        Self {
            finished: FinishedInput {
                scope: "client".into(),
                key: key.into(),
                fingerprint: "fingerprint".into(),
                result: serde_json::json!({"acceptedBytes": 1}),
                actor: serde_json::json!({"role": "human_local"}),
            },
            operation_key: operation_key.into(),
            queued_ms,
        }
    }
}

impl Core {
    /// Queues the outcome `finished` of an input the supervisor accepted. The actor was taken
    /// when the request was dispatched, so a connection that closed meanwhile loses nothing.
    pub(crate) fn queue_input_receipt(&mut self, finished: FinishedInput, operation_key: String) {
        // A retry that was sent again before the outcome was recorded queues it once.
        if self
            .queued_input_outcome(&finished.scope, &finished.key)
            .is_some()
        {
            return;
        }
        let queued_ms = self.now_ms();
        self.input_receipts.push(Waiting {
            finished,
            operation_key,
            queued_ms,
        });
    }

    /// The outcome of the input `key` when it waits to be recorded: a retry is answered with it
    /// and goes nowhere near the supervisor, which could have forgotten the operation by the time
    /// a write of the retry reached it (the confirmation follows the record).
    pub(crate) fn queued_input_outcome(&self, scope: &str, key: &str) -> Option<&FinishedInput> {
        self.input_receipts
            .iter()
            .map(Waiting::finished)
            .find(|finished| finished.scope == scope && finished.key == key)
    }

    /// Tells the supervisor it may forget the operations whose outcome has just been recorded.
    pub(crate) fn confirm_inputs(&mut self, operation_keys: Vec<String>) {
        for operation_key in operation_keys {
            self.send_later(Request::ConfirmInput { operation_key });
        }
    }

    /// The supervisor holds the operation of an input whose intent the store holds: either its
    /// client is about to retry it, or the engine ended before it recorded the outcome and nobody
    /// will. The sweep looks at it, and forgets it once the store says the intent is older than
    /// [`ORPHAN_AFTER_S`].
    pub(crate) fn watch_orphan_input(&mut self, operation_key: String) {
        let now = self.now_ms();
        self.orphan_inputs.entry(operation_key).or_insert(now);
    }

    /// Forgets the intents nobody retried within [`ORPHAN_AFTER_S`] of being recorded, and tells
    /// the supervisor to forget their operations. The age is the store's, so a slow client's retry
    /// is not cut short by an engine that started late. An outcome that waits to be recorded
    /// confirms its own. An entry stays until its intent is gone, and a failure is tried again.
    fn sweep_orphan_inputs(&mut self) -> Result<()> {
        let now = self.now_ms();
        let due: Vec<String> = self
            .orphan_inputs
            .iter()
            .filter(|(_, next)| now >= **next || **next - now > ORPHAN_RECHECK_MS)
            .map(|(operation_key, _)| operation_key.clone())
            .collect();
        let recorded_by = self.now_unix() - ORPHAN_AFTER_S;
        let mut failure = None;
        for operation_key in due {
            if self
                .input_receipts
                .iter()
                .any(|waiting| waiting.operation_key == operation_key)
            {
                self.orphan_inputs.remove(&operation_key);
                continue;
            }
            match self.store.drop_input_intent(&operation_key, recorded_by) {
                Ok(true) => {
                    self.orphan_inputs.remove(&operation_key);
                    self.confirm_inputs(vec![operation_key]);
                }
                Ok(false) => {
                    // Younger than the grace, or already gone (a retry completed it).
                    match self.store.input_intent_operation_pending(&operation_key) {
                        Ok(true) => {
                            self.orphan_inputs
                                .insert(operation_key, now + ORPHAN_RECHECK_MS);
                        }
                        Ok(false) => {
                            self.orphan_inputs.remove(&operation_key);
                        }
                        Err(error) => failure = Some(errors::storage(error)),
                    }
                }
                Err(error) => {
                    self.orphan_inputs
                        .insert(operation_key, now + ORPHAN_RECHECK_MS);
                    failure = Some(errors::storage(error));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// When the tick must record the outcome that has waited longest, and look at the intents
    /// nobody retried.
    pub(crate) fn input_receipts_due(&self, next: &mut Next, now: i64) {
        if let Some(waiting) = self.input_receipts.first() {
            next.at_ms(now, waiting.queued_ms.saturating_add(LINGER_MS));
        }
        for due in self.orphan_inputs.values() {
            next.at_ms(now, *due);
        }
    }

    /// The tick: records the outcomes that have waited for the next input long enough.
    pub(crate) fn flush_input_receipts(&mut self) -> Result<()> {
        let now = self.now_ms();
        // A clock that went back makes an outcome wait for ever, so it counts as due.
        let due = self
            .input_receipts
            .first()
            .is_some_and(|waiting| now < waiting.queued_ms || now - waiting.queued_ms >= LINGER_MS);
        if due {
            self.record_input_receipts()?;
        }
        self.sweep_orphan_inputs()
    }

    /// The engine is stopping: records every outcome that waits.
    pub(crate) fn flush_all_input_receipts(&mut self) -> Result<()> {
        if self.input_receipts.is_empty() {
            return Ok(());
        }
        self.record_input_receipts()
    }

    fn record_input_receipts(&mut self) -> Result<()> {
        let finished: Vec<_> = self
            .input_receipts
            .iter()
            .map(|waiting| waiting.finished.clone())
            .collect();
        crate::ports::StorePort::finish_inputs(&mut self.store, &finished, None)
            .map_err(errors::storage)?;
        let confirmed = self
            .input_receipts
            .drain(..)
            .map(|waiting| waiting.operation_key)
            .collect();
        self.confirm_inputs(confirmed);
        Ok(())
    }
}

/// Splits off the operation keys of the outcomes a write just recorded.
pub(crate) fn operation_keys(waiting: Vec<Waiting>) -> Vec<String> {
    waiting
        .into_iter()
        .map(|waiting| waiting.operation_key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::Outcome;
    use crate::{
        supervisor_handle::tests::{Stub, core_with_stub, open_core, request, wait_for},
        supervisor_link::tests::Manual,
        test_panes::Answering,
    };
    use serde_json::json;
    use soyeht_ptyd::wire::Response;
    use std::sync::{Arc, atomic::AtomicI64};
    use std::{thread, time::Duration};

    /// A core whose supervisor records the operations it is told to forget, and whose clock the
    /// test moves.
    fn core_confirming() -> (
        Core,
        std::path::PathBuf,
        Arc<std::sync::Mutex<Vec<String>>>,
        Arc<Manual>,
    ) {
        let directory =
            std::env::temp_dir().join(format!("soyeht-receipts-{}", uuid::Uuid::new_v4()));
        let mut core = Core::open(&directory).unwrap();
        let confirmed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = Arc::clone(&confirmed);
        core.link_supervisor(Box::new(Answering(move |request| {
            if let Request::ConfirmInput { operation_key } = request {
                record.lock().unwrap().push(operation_key);
            }
            Ok(Response::Ok)
        })));
        core.background = crate::offload::Background::inline();
        let clock = Arc::new(Manual(AtomicI64::new(1_000)));
        core.clock = clock.clone();
        (core, directory, confirmed, clock)
    }

    #[test]
    fn an_outcome_that_no_input_follows_is_recorded_alone_after_the_linger_and_then_confirmed() {
        let (mut core, directory, confirmed, clock) = core_confirming();
        core.input_receipts
            .push(Waiting::for_test("a", "op-a", 1_000));
        core.input_receipts
            .push(Waiting::for_test("b", "op-b", 1_100));

        // Inside the linger nothing is written: the next input may carry them.
        clock
            .0
            .store(1_000 + LINGER_MS - 1, std::sync::atomic::Ordering::SeqCst);
        core.flush_input_receipts().unwrap();
        assert_eq!(core.input_receipts.len(), 2);
        assert!(
            core.store
                .idempotency_receipt("client", "a")
                .unwrap()
                .is_none()
        );

        // Past it, both are recorded in one write and the supervisor is told to forget them.
        clock
            .0
            .store(1_000 + LINGER_MS, std::sync::atomic::Ordering::SeqCst);
        core.flush_input_receipts().unwrap();
        assert!(core.input_receipts.is_empty());
        for key in ["a", "b"] {
            let receipt = core
                .store
                .idempotency_receipt("client", key)
                .unwrap()
                .unwrap();
            assert_eq!(receipt.result, serde_json::json!({"acceptedBytes": 1}));
        }
        assert_eq!(*confirmed.lock().unwrap(), ["op-a", "op-b"]);
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn stopping_the_engine_records_what_waits_at_once() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-receipts-{}", uuid::Uuid::new_v4()));
        let mut core = Core::open(&directory).unwrap();
        core.flush_all_input_receipts().unwrap();
        core.input_receipts
            .push(Waiting::for_test("a", "op-a", core.now_ms()));

        core.flush_all_input_receipts().unwrap();

        assert!(core.input_receipts.is_empty());
        assert!(
            core.store
                .idempotency_receipt("client", "a")
                .unwrap()
                .is_some()
        );
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn an_intent_nobody_retries_is_forgotten_a_minute_after_it_was_recorded_and_its_operation_confirmed()
     {
        let (mut core, directory, confirmed, clock) = core_confirming();
        let intent = |operation_key: &str| soyeht_store::domain::InputIntent {
            fingerprint: "fingerprint".into(),
            operation_key: operation_key.into(),
            supervisor_id: "supervisor".into(),
            instance_id: "instance".into(),
        };
        // The intents are recorded now (the real clock of the store); the engine's clock is the
        // test's, which starts at a time a new engine might have first seen them.
        let recorded = chrono::Utc::now().timestamp();
        clock
            .0
            .store(recorded * 1_000, std::sync::atomic::Ordering::SeqCst);
        for (key, operation) in [("a", "op-a"), ("b", "op-b")] {
            core.store
                .finish_inputs(&[], Some(("client", key, &intent(operation))))
                .unwrap();
            core.watch_orphan_input(operation.into());
        }
        // The outcome of one of them waits: it confirms its own.
        core.input_receipts
            .push(Waiting::for_test("b", "op-b", recorded * 1_000));

        // A new engine that first sees the intents a minute after they were recorded forgets
        // them at once; one that sees them younger waits for the rest of the minute.
        clock.0.store(
            (recorded + ORPHAN_AFTER_S - 1) * 1_000,
            std::sync::atomic::Ordering::SeqCst,
        );
        core.flush_input_receipts().unwrap();
        assert!(core.store.input_intent_operation_pending("op-a").unwrap());
        assert!(
            core.orphan_inputs.contains_key("op-a"),
            "it is looked at again"
        );

        clock.0.store(
            (recorded + ORPHAN_AFTER_S + ORPHAN_RECHECK_MS / 1_000) * 1_000,
            std::sync::atomic::Ordering::SeqCst,
        );
        core.flush_input_receipts().unwrap();
        assert!(!core.store.input_intent_operation_pending("op-a").unwrap());
        assert!(!core.orphan_inputs.contains_key("op-a"));
        let confirmed = confirmed.lock().unwrap();
        assert!(confirmed.contains(&"op-a".to_owned()), "{confirmed:?}");
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn the_confirmation_of_an_outcome_recorded_at_shutdown_reaches_the_supervisor() {
        // Jobs run on their own threads, as in the engine, and no tick is left to start the one
        // that sends the confirmation.
        let (mut core, directory, confirmed, _) = core_confirming();
        core.background = crate::offload::Background::default();
        core.input_receipts.push(Waiting::for_test("a", "op-a", 0));

        core.flush_all_input_receipts().unwrap();
        for _ in 0..3 {
            let Some(done) = core.send_follow_ups_before_exit() else {
                break;
            };
            assert!(done.wait(Duration::from_secs(30)));
        }

        assert_eq!(*confirmed.lock().unwrap(), ["op-a"]);
        assert!(core.send_follow_ups_before_exit().is_none());
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn an_outcome_is_due_when_the_clock_went_back() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-receipts-{}", uuid::Uuid::new_v4()));
        let mut core = Core::open(&directory).unwrap();
        let clock = Arc::new(Manual(AtomicI64::new(5_000)));
        core.clock = clock.clone();
        core.input_receipts
            .push(Waiting::for_test("a", "op-a", 5_000));
        clock.0.store(1_000, std::sync::atomic::Ordering::SeqCst);

        core.flush_input_receipts().unwrap();

        assert!(core.input_receipts.is_empty());
        crate::test_paths::remove_after(core, directory);
    }

    #[test]
    fn a_retry_while_the_outcome_waits_is_answered_from_it_and_the_supervisor_writes_once() {
        let (mut core, root, ids, asked) = core_with_stub();
        let (connection, pane) = ids.split_once('|').unwrap();
        let clock = Arc::new(Manual(std::sync::atomic::AtomicI64::new(1_000)));
        core.clock = clock.clone();
        let typed = |id| {
            request(
                id,
                "pane.input",
                json!({"paneId":pane,"bytes":"bHMK","idempotencyKey":"typed"}),
            )
        };
        let answer = |core: &mut Core, id| match core.dispatch_inline(connection, typed(id)) {
            Ok(Outcome::Reply(reply)) => reply,
            other => panic!("pane.input did not reply: {:?}", other.err()),
        };

        let first = answer(&mut core, 2);
        assert_eq!(first["acceptedBytes"], 4);
        // The client retries with the same key while the outcome waits for the next write: it is
        // answered from the outcome, and the supervisor is not asked again.
        assert_eq!(answer(&mut core, 3), first);
        assert_eq!(*asked.lock().unwrap(), ["input:bHMK"]);

        // The outcome is recorded after the linger and the supervisor told to forget the operation;
        // a retry after that is answered by the database.
        clock
            .0
            .store(1_000 + LINGER_MS, std::sync::atomic::Ordering::SeqCst);
        core.flush_input_receipts().unwrap();
        assert_eq!(*asked.lock().unwrap(), ["input:bHMK", "confirm"]);
        assert_eq!(answer(&mut core, 4), first);
        assert_eq!(*asked.lock().unwrap(), ["input:bHMK", "confirm"]);

        // The same key with other bytes is refused, whichever record it meets.
        let other = core.dispatch_inline(
            connection,
            request(
                5,
                "pane.input",
                json!({"paneId":pane,"bytes":"bHMM","idempotencyKey":"typed"}),
            ),
        );
        assert!(other.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_confirmation_waits_for_the_inputs_on_their_way() {
        let (mut core, root, _) = open_core();
        let (stub, _, asked) = Stub::new(Duration::ZERO);
        core.link_supervisor(Box::new(stub));
        // An input to the process is on its way: it holds the turn of the process.
        let on_its_way = core.turn(&["shell-instance"]);
        core.send_later(Request::ConfirmInput {
            operation_key: "op".into(),
        });

        thread::sleep(Duration::from_millis(300));
        assert!(
            asked.lock().unwrap().is_empty(),
            "the confirmation overtook the input"
        );
        drop(on_its_way);
        wait_for("the confirmation was never sent", || {
            asked.lock().unwrap().len() == 1
        });
        assert_eq!(*asked.lock().unwrap(), ["confirm"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
