use crate::{
    core::Core,
    errors::{self, Result},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use soyeht_protocol::ErrorKind;
use soyeht_ptyd::wire::{Request, Response};
use uuid::Uuid;

impl Core {
    pub fn pane_input(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let id = params["paneId"].as_str().expect("pane");
        let saved = self
            .panes
            .get(id)
            .filter(|p| !p.stored.closed)
            .ok_or_else(|| errors::error(ErrorKind::NotFound, "Pane not found"))?
            .stored
            .clone();
        let instance = saved
            .instance_id
            .ok_or_else(|| errors::error(ErrorKind::PaneBusy, "Pane is recovering"))?;
        let supervisor = saved.supervisor_id.expect("adopted supervisor");
        let scope = self.clients[connection].scope.clone();
        let key = params["idempotencyKey"].as_str().expect("key");
        let fingerprint = STANDARD.encode(crate::auth::token_hash(
            &json!(["pane.input", params]).to_string(),
        ));
        let prior:Option<(String,String,String,String)>=self.store.connection().query_row("SELECT fingerprint,operation_key,supervisor_id,instance_id FROM input_intents WHERE scope=?1 AND key=?2",params![scope,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e|errors::storage(e.into()))?;
        let operation_key = if let Some((prior, key, old_supervisor, old_instance)) = prior {
            if prior != fingerprint {
                return Err(errors::error(
                    ErrorKind::IdempotencyConflict,
                    "Input key reused with different content",
                ));
            }
            if old_supervisor != supervisor || old_instance != instance {
                return Err(errors::error(
                    ErrorKind::ResyncRequired,
                    "Previous input outcome is unknown; it will not be repeated in a replacement shell",
                ));
            }
            key
        } else {
            let operation = Uuid::new_v4().to_string();
            // SQLite FULL commits this intent before any irreversible PTY write.
            // Only a digest is retained; input bytes may contain credentials.
            self.store.connection().execute("INSERT INTO input_intents(scope,key,fingerprint,operation_key,supervisor_id,instance_id) VALUES (?1,?2,?3,?4,?5,?6)",params![scope,key,fingerprint,operation,supervisor,instance]).map_err(|e|errors::storage(e.into()))?;
            operation
        };
        let Response::Accepted { bytes } = self.pty_request(Request::Input {
            instance_id: instance,
            bytes: params["bytes"].as_str().expect("bytes").into(),
            operation_key: operation_key.clone(),
            fingerprint,
        })?
        else {
            return Err(errors::error(
                ErrorKind::PtydUnavailable,
                "Invalid input receipt",
            ));
        };
        let result = self
            .mutate(connection, "pane.input", params, |tx| {
                tx.execute(
                    "DELETE FROM input_intents WHERE scope=?1 AND key=?2",
                    params![scope, key],
                )?;
                Ok((json!({"acceptedBytes":bytes}), vec![]))
            })?
            .result;
        // Once the engine receipt is durable, retries are answered by SQLite.
        let _ = self.pty_request(Request::ConfirmInput { operation_key });
        Ok(result)
    }
}
