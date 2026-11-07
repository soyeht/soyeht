use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    input_receipts::{Waiting, operation_keys},
    offload::Offload,
    ports::StorePort,
    supervisor_handle,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_protocol::method::PANE_INPUT;
use soyeht_ptyd::wire::Request;
use soyeht_store::domain::InputIntent;
use uuid::Uuid;

/// The operation key of the input `key`: the one of its intent when a retry finds it, or a new
/// intent, recorded together with the outcomes that wait for a write (they are taken out of
/// `waiting`, and their operation keys returned to be confirmed).
fn resolve_intent(
    store: &mut impl StorePort,
    waiting: &mut Vec<Waiting>,
    (scope, key): (&str, &str),
    intent: InputIntent,
) -> Result<(String, Vec<String>)> {
    let prior = store.input_intent(scope, key).map_err(errors::storage)?;
    if let Some(prior) = prior {
        if prior.fingerprint != intent.fingerprint {
            return Err(errors::idempotency_conflict(
                "Input key reused with different content",
            ));
        }
        if prior.supervisor_id != intent.supervisor_id || prior.instance_id != intent.instance_id {
            return Err(errors::resync_required(
                "Previous input outcome is unknown; it will not be repeated in a replacement shell",
            ));
        }
        return Ok((prior.operation_key, Vec::new()));
    }
    let finished: Vec<_> = waiting
        .iter()
        .map(|waiting| waiting.finished().clone())
        .collect();
    store
        .finish_inputs(&finished, Some((scope, key, &intent)))
        .map_err(errors::storage)?;
    Ok((
        intent.operation_key,
        operation_keys(std::mem::take(waiting)),
    ))
}

impl Core {
    /// `pane.input`: the durable intent is recorded under the lock, the bytes go to the
    /// supervisor with no lock, and the receipt is made durable under it (ADR 0117). The
    /// connection answers its next request only after this one, so inputs keep their order.
    pub fn pane_input(&mut self, connection: &str, params: &Value) -> Result<Outcome> {
        let id = errors::param_str(params, "paneId")?;
        let saved = self
            .panes
            .get(id)
            .filter(|p| !p.stored.closed)
            .ok_or_else(|| errors::not_found("Pane not found"))?
            .stored
            .clone();
        if saved.pane.mode != soyeht_protocol::PaneMode::Terminal {
            return Err(errors::invalid("Raw input requires a terminal pane"));
        }
        let instance = saved
            .instance_id
            .ok_or_else(|| errors::pane_busy("Pane is recovering"))?;
        let supervisor = errors::invariant(
            saved.supervisor_id,
            "Terminal process has no adopted supervisor",
        )?;
        let scope = self.clients[connection].scope.clone();
        let key = errors::param_str(params, "idempotencyKey")?.to_owned();
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!([PANE_INPUT, params]).to_string(),
        ));
        let bytes = errors::param_str(params, "bytes")?.to_owned();
        // The durable intent precedes the irreversible supervisor write. Only
        // a digest is retained; input bytes may contain credentials.
        let (operation_key, recorded) = resolve_intent(
            &mut self.store,
            &mut self.input_receipts,
            (&scope, &key),
            InputIntent {
                fingerprint: fingerprint.clone(),
                operation_key: Uuid::new_v4().to_string(),
                supervisor_id: supervisor,
                instance_id: instance.clone(),
            },
        )?;
        self.confirm_inputs(recorded);
        let link = self.supervisor()?;
        let used = link.clone();
        // The input reaches the supervisor after every send to this process that was made
        // before it, from whichever thread: the queued ones go in front of it, and its turn
        // follows the sends already on their way (and the inputs dispatched before it).
        let carried = self.take_later_for(&instance);
        let turn = self.turn(&[&instance]);
        let receipt_fingerprint = fingerprint.clone();
        let request = Request::Input {
            instance_id: instance,
            bytes,
            operation_key: operation_key.clone(),
            fingerprint,
        };
        let connection = connection.to_owned();
        Ok(Outcome::Offload(Offload::new(
            "Supervisor input timed out",
            supervisor_handle::DEADLINE,
            {
                let carried = carried.clone();
                move || Ok(link.send_input(&turn, &carried, request))
            },
            move |core, sent| {
                // Whatever the work did not take is sent after all, by the follow-up job. A
                // receipt of a link that was replaced is no receipt: the client is told.
                core.requeue_later(&carried);
                let sent = sent?;
                let bytes = core
                    .settle_input(&used, &carried, sent)?
                    .ok_or_else(|| errors::ptyd_unavailable("Supervisor link was replaced"))?;
                let result = json!({"acceptedBytes":bytes});
                // Once the engine receipt is durable, retries are answered by SQLite and the
                // supervisor is told to forget the operation: both follow the next write of
                // inputs (see `input_receipts`).
                core.queue_input_receipt(
                    &connection,
                    (&scope, &key, &receipt_fingerprint),
                    &result,
                    operation_key,
                )?;
                Ok(Outcome::Reply(result))
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        call_supervisor,
        testing::{FakeSupervisor, MemoryStore},
    };
    use soyeht_protocol::ErrorKind;
    use soyeht_ptyd::wire::Response;
    use std::collections::VecDeque;

    fn intent(supervisor: &str, instance: &str, operation_key: &str) -> InputIntent {
        InputIntent {
            fingerprint: "fingerprint".into(),
            operation_key: operation_key.into(),
            supervisor_id: supervisor.into(),
            instance_id: instance.into(),
        }
    }

    #[test]
    fn input_rule_reserves_once_and_rejects_ambiguous_replacement() {
        let mut store = MemoryStore::default();
        let mut waiting = Vec::new();
        let (first, _) = resolve_intent(
            &mut store,
            &mut waiting,
            ("client", "key"),
            intent("supervisor-1", "instance-1", "operation-1"),
        )
        .unwrap();
        assert_eq!(first, "operation-1");
        let (repeated, _) = resolve_intent(
            &mut store,
            &mut waiting,
            ("client", "key"),
            intent("supervisor-1", "instance-1", "unused"),
        )
        .unwrap();
        assert_eq!(repeated, first);

        let error = resolve_intent(
            &mut store,
            &mut waiting,
            ("client", "key"),
            intent("supervisor-2", "instance-2", "unused"),
        )
        .unwrap_err();
        assert_eq!(errors::kind(&error), ErrorKind::ResyncRequired);
        // Only the first wrote: a retry finds the intent and records nothing.
        assert_eq!(store.input_writes, [(0, true)]);

        let mut supervisor = FakeSupervisor {
            responses: VecDeque::from([Ok(Response::Accepted { bytes: 4 })]),
            ..FakeSupervisor::default()
        };
        let response = call_supervisor(
            &mut supervisor,
            Request::Input {
                instance_id: "instance-1".into(),
                bytes: "SAFE".into(),
                operation_key: first,
                fingerprint: "fingerprint".into(),
            },
        )
        .unwrap();
        assert!(matches!(response, Response::Accepted { bytes: 4 }));
        assert_eq!(supervisor.requests.len(), 1);
    }

    #[test]
    fn the_outcomes_that_wait_are_recorded_with_the_intent_of_the_next_input() {
        let mut store = MemoryStore::default();
        let mut waiting = vec![
            Waiting::for_test("a", "op-a", 0),
            Waiting::for_test("b", "op-b", 0),
        ];

        let (_, recorded) = resolve_intent(
            &mut store,
            &mut waiting,
            ("client", "c"),
            intent("supervisor", "instance", "op-c"),
        )
        .unwrap();

        // One write for the two outcomes and the new intent, and the supervisor may now forget
        // the operations whose outcome it carried.
        assert_eq!(store.input_writes, [(2, true)]);
        assert_eq!(recorded, ["op-a", "op-b"]);
        assert!(waiting.is_empty());

        // A retry of an input that is still waiting for its outcome finds its intent: no write,
        // and the outcomes that wait stay.
        let mut waiting = vec![Waiting::for_test("d", "op-d", 0)];
        let (found, recorded) = resolve_intent(
            &mut store,
            &mut waiting,
            ("client", "c"),
            intent("supervisor", "instance", "unused"),
        )
        .unwrap();
        assert_eq!(found, "op-c");
        assert!(recorded.is_empty());
        assert_eq!(waiting.len(), 1);
        assert_eq!(store.input_writes.len(), 1);
    }
}
