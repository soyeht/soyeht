use crate::{
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use soyeht_ptyd::wire::{Request, Response};
use soyeht_store::domain::InputIntent;
use uuid::Uuid;

fn resolve_intent(
    store: &mut impl StorePort,
    scope: &str,
    key: &str,
    fingerprint: &str,
    supervisor_id: &str,
    instance_id: &str,
    new_operation_key: String,
) -> Result<String> {
    let prior = store.input_intent(scope, key).map_err(errors::storage)?;
    if let Some(prior) = prior {
        if prior.fingerprint != fingerprint {
            return Err(errors::idempotency_conflict(
                "Input key reused with different content",
            ));
        }
        if prior.supervisor_id != supervisor_id || prior.instance_id != instance_id {
            return Err(errors::resync_required(
                "Previous input outcome is unknown; it will not be repeated in a replacement shell",
            ));
        }
        return Ok(prior.operation_key);
    }
    store
        .record_input_intent(
            scope,
            key,
            &InputIntent {
                fingerprint: fingerprint.into(),
                operation_key: new_operation_key.clone(),
                supervisor_id: supervisor_id.into(),
                instance_id: instance_id.into(),
            },
        )
        .map_err(errors::storage)?;
    Ok(new_operation_key)
}

impl Core {
    pub fn pane_input(&mut self, connection: &str, params: &Value) -> Result<Value> {
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
        let key = errors::param_str(params, "idempotencyKey")?;
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!(["pane.input", params]).to_string(),
        ));
        // The durable intent precedes the irreversible supervisor write. Only
        // a digest is retained; input bytes may contain credentials.
        let operation_key = resolve_intent(
            &mut self.store,
            &scope,
            key,
            &fingerprint,
            &supervisor,
            &instance,
            Uuid::new_v4().to_string(),
        )?;
        let Response::Accepted { bytes } = self.pty_request(Request::Input {
            instance_id: instance,
            bytes: errors::param_str(params, "bytes")?.into(),
            operation_key: operation_key.clone(),
            fingerprint,
        })?
        else {
            return Err(errors::ptyd_unavailable("Invalid input receipt"));
        };
        let result = self
            .mutate(connection, "pane.input", params, |tx| {
                tx.remove_input_intent(&scope, key)?;
                Ok((json!({"acceptedBytes":bytes}), vec![]))
            })?
            .result;
        // Once the engine receipt is durable, retries are answered by SQLite.
        let _ = self.pty_request(Request::ConfirmInput { operation_key });
        Ok(result)
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
    use std::collections::VecDeque;

    #[test]
    fn input_rule_reserves_once_and_rejects_ambiguous_replacement() {
        let mut store = MemoryStore::default();
        let first = resolve_intent(
            &mut store,
            "client",
            "key",
            "fingerprint",
            "supervisor-1",
            "instance-1",
            "operation-1".into(),
        )
        .unwrap();
        assert_eq!(first, "operation-1");
        let repeated = resolve_intent(
            &mut store,
            "client",
            "key",
            "fingerprint",
            "supervisor-1",
            "instance-1",
            "unused".into(),
        )
        .unwrap();
        assert_eq!(repeated, first);

        let error = resolve_intent(
            &mut store,
            "client",
            "key",
            "fingerprint",
            "supervisor-2",
            "instance-2",
            "unused".into(),
        )
        .unwrap_err();
        assert_eq!(errors::kind(&error), ErrorKind::ResyncRequired);

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
}
