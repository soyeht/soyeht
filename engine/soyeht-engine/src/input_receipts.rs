//! The outcome of a terminal input is made durable with the next write of inputs, not alone
//! (ADR 0147, step R).
//!
//! A `pane.input` records its intent before the irreversible write to the supervisor and its
//! outcome after it: two commits, and a commit is a flush of the disk under the Core lock. The
//! outcome only lets a retry with the same key be answered from the database. Until it is
//! recorded, the intent is still there: a retry finds it, sends the same operation key, and the
//! supervisor, which keeps the operation until the engine confirms it, answers once. So the reply
//! may go before the outcome is durable, and the outcome waits for the next write of an input
//! (the commit of its intent carries it) or for a quiet moment ([`LINGER_MS`]), and the supervisor
//! is told to forget the operation only after that.
//!
//! Nothing weaker is promised than before: the intent is durable before the write to the terminal,
//! as it was, and the confirmation of an operation still follows the commit of its outcome.

use crate::{
    core::Core,
    errors::{self, Result},
};
use soyeht_ptyd::wire::Request;
use soyeht_store::domain::FinishedInput;

/// How long an outcome waits for the next input before it is recorded alone. A person types
/// faster than this, so the keystrokes of a burst share their commits.
pub(crate) const LINGER_MS: i64 = 250;

/// How long an intent whose operation the supervisor holds is left for its client to retry before
/// it is forgotten: a client retries within seconds.
const ORPHAN_AFTER_MS: i64 = 60_000;

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
    /// will. It is looked at again after [`ORPHAN_AFTER_MS`].
    pub(crate) fn watch_orphan_input(&mut self, operation_key: String) {
        let now = self.now_ms();
        self.orphan_inputs.entry(operation_key).or_insert(now);
    }

    /// Forgets the intents nobody retried within [`ORPHAN_AFTER_MS`] of the engine seeing them,
    /// and tells the supervisor to forget their operations. An outcome that waits to be recorded
    /// confirms its own.
    fn sweep_orphan_inputs(&mut self) -> Result<()> {
        let now = self.now_ms();
        let due: Vec<String> = self
            .orphan_inputs
            .iter()
            .filter(|(_, since)| now < **since || now - **since >= ORPHAN_AFTER_MS)
            .map(|(operation_key, _)| operation_key.clone())
            .collect();
        for operation_key in due {
            self.orphan_inputs.remove(&operation_key);
            if self
                .input_receipts
                .iter()
                .any(|waiting| waiting.operation_key == operation_key)
            {
                continue;
            }
            if self
                .store
                .drop_input_intent(&operation_key)
                .map_err(errors::storage)?
            {
                self.confirm_inputs(vec![operation_key]);
            }
        }
        Ok(())
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
    use crate::{supervisor_link::tests::Manual, test_panes::Answering};
    use soyeht_ptyd::wire::Response;
    use std::sync::{Arc, atomic::AtomicI64};

    #[test]
    fn an_outcome_that_no_input_follows_is_recorded_alone_after_the_linger_and_then_confirmed() {
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
    fn an_intent_nobody_retries_is_forgotten_after_a_minute_and_its_operation_confirmed() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-orphans-{}", uuid::Uuid::new_v4()));
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
        let intent = |operation_key: &str| soyeht_store::domain::InputIntent {
            fingerprint: "fingerprint".into(),
            operation_key: operation_key.into(),
            supervisor_id: "supervisor".into(),
            instance_id: "instance".into(),
        };
        for (key, operation) in [("a", "op-a"), ("b", "op-b")] {
            core.store
                .finish_inputs(&[], Some(("client", key, &intent(operation))))
                .unwrap();
            core.watch_orphan_input(operation.into());
        }
        // The outcome of one of them waits: it confirms its own.
        core.input_receipts
            .push(Waiting::for_test("b", "op-b", 1_000));

        clock.0.store(
            1_000 + ORPHAN_AFTER_MS - 1,
            std::sync::atomic::Ordering::SeqCst,
        );
        core.flush_input_receipts().unwrap();
        assert!(core.store.input_intent_operation_pending("op-a").unwrap());

        clock
            .0
            .store(1_000 + ORPHAN_AFTER_MS, std::sync::atomic::Ordering::SeqCst);
        core.flush_input_receipts().unwrap();
        assert!(!core.store.input_intent_operation_pending("op-a").unwrap());
        assert!(core.orphan_inputs.is_empty());
        let confirmed = confirmed.lock().unwrap();
        assert!(confirmed.contains(&"op-a".to_owned()), "{confirmed:?}");
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
}
