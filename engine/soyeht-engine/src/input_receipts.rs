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
use serde_json::Value;
use soyeht_ptyd::wire::Request;
use soyeht_store::domain::FinishedInput;

/// How long an outcome waits for the next input before it is recorded alone. A person types
/// faster than this, so the keystrokes of a burst share their commits.
pub(crate) const LINGER_MS: i64 = 250;

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
    /// Queues the outcome of the input `key` of `connection`, which the supervisor accepted.
    pub(crate) fn queue_input_receipt(
        &mut self,
        connection: &str,
        input: (&str, &str, &str),
        result: &Value,
        operation_key: String,
    ) -> Result<()> {
        let (scope, key, fingerprint) = input;
        let actor = self.actor_json(connection)?;
        // A retry that was sent again before the outcome was recorded queues it once.
        if self
            .input_receipts
            .iter()
            .any(|waiting| waiting.finished.scope == scope && waiting.finished.key == key)
        {
            return Ok(());
        }
        let queued_ms = self.now_ms();
        self.input_receipts.push(Waiting {
            finished: FinishedInput {
                scope: scope.into(),
                key: key.into(),
                fingerprint: fingerprint.into(),
                result: result.clone(),
                actor,
            },
            operation_key,
            queued_ms,
        });
        Ok(())
    }

    /// Tells the supervisor it may forget the operations whose outcome has just been recorded.
    pub(crate) fn confirm_inputs(&mut self, operation_keys: Vec<String>) {
        for operation_key in operation_keys {
            self.send_later(Request::ConfirmInput { operation_key });
        }
    }

    /// The tick: records the outcomes that have waited for the next input long enough.
    pub(crate) fn flush_input_receipts(&mut self) -> Result<()> {
        let now = self.now_ms();
        let due = self
            .input_receipts
            .first()
            .is_some_and(|waiting| now - waiting.queued_ms >= LINGER_MS);
        if due {
            self.record_input_receipts()?;
        }
        Ok(())
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
}
