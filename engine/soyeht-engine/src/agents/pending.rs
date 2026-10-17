use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    model::{self, Waiter},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::RpcError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserOperation {
    pub scope: String,
    pub key: String,
}

pub struct Completion {
    pub user: UserOperation,
    pub result: std::result::Result<Value, RpcError>,
}

impl Completion {
    pub fn stored(&self) -> Value {
        match &self.result {
            Ok(value) => value.clone(),
            Err(error) => json!({"_nativeError":error}),
        }
    }
}

impl Core {
    pub fn native_rpc_result(
        &mut self,
        connection: &str,
        id: Value,
        method: &str,
        params: &Value,
        value: Value,
    ) -> Result<Outcome> {
        if let Some(error) = value.get("_nativeError") {
            let error = serde_json::from_value::<RpcError>(error.clone())
                .map_err(|_| errors::internal("Invalid stored native response"))?;
            return Err(errors::Error::RecordedRpc(error));
        }
        if value.get("_nativePending").is_none() {
            return Ok(Outcome::Reply(value));
        }
        let scope = self.clients[connection].scope.clone();
        let key = errors::param_str(params, "idempotencyKey")?.to_owned();
        let waiters = self.native_waiters.entry((scope, key)).or_default();
        if !waiters
            .iter()
            .any(|waiter| waiter.connection == connection && waiter.request_id == id)
        {
            if waiters.len() >= 128 {
                return Err(errors::pane_busy("Native operation waiter limit reached"));
            }
            waiters.push(Waiter {
                connection: connection.into(),
                request_id: id,
                method: method.into(),
            });
        }
        Ok(Outcome::Queued)
    }

    pub fn publish_native_completion(&mut self, completion: &Completion) -> Result<()> {
        let waiters = self
            .native_waiters
            .remove(&(completion.user.scope.clone(), completion.user.key.clone()))
            .unwrap_or_default();
        for waiter in waiters {
            if let Some(client) = self
                .clients
                .get(&waiter.connection)
                .filter(|client| client.scope == completion.user.scope && client.actor.is_some())
            {
                let frame = match &completion.result {
                    Ok(value) => model::response(&waiter.method, waiter.request_id, value.clone())?,
                    Err(error) => model::failure(waiter.request_id, error.clone())?,
                };
                client.outbox.send(frame);
            }
        }
        Ok(())
    }
}
