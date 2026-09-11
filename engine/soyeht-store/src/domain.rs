//! Named persistence operations used by the engine.
//!
//! SQL and driver types stay inside `soyeht-store`. Mutations receive a
//! [`Change`] so domain rows and their public events commit atomically.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use soyeht_protocol::{Pane, Workspace};

#[derive(Clone, Debug, PartialEq)]
pub struct PaneRecord {
    pub pane: Pane,
    pub closed: bool,
    pub instance_key: String,
    pub instance_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub sequence_base: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputIntent {
    pub fingerprint: String,
    pub operation_key: String,
    pub supervisor_id: String,
    pub instance_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IdempotencyReceipt {
    pub fingerprint: String,
    pub result: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub public_key: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientFocus {
    pub workspace_id: Option<String>,
    pub pane_id: Option<String>,
}

fn pane_row(row: &Row<'_>) -> rusqlite::Result<PaneRecord> {
    let payload: String = row.get(0)?;
    let pane = serde_json::from_str(&payload).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let raw_base: String = row.get(5)?;
    let sequence_base = raw_base.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(PaneRecord {
        pane,
        closed: row.get(1)?,
        instance_key: row.get(2)?,
        instance_id: row.get(3)?,
        supervisor_id: row.get(4)?,
        sequence_base,
    })
}

impl Store {
    pub fn workspace(&self, id: &str) -> Result<Option<Workspace>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM workspaces WHERE id=?1 AND closed=0",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn panes(&self) -> Result<Vec<PaneRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT payload,closed,instance_key,instance_id,supervisor_id,sequence_base FROM panes ORDER BY rowid",
        )?;
        Ok(statement
            .query_map([], pane_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .connection
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn input_intent(&self, scope: &str, key: &str) -> Result<Option<InputIntent>> {
        Ok(self
            .connection
            .query_row(
                "SELECT fingerprint,operation_key,supervisor_id,instance_id FROM input_intents WHERE scope=?1 AND key=?2",
                params![scope, key],
                |row| {
                    Ok(InputIntent {
                        fingerprint: row.get(0)?,
                        operation_key: row.get(1)?,
                        supervisor_id: row.get(2)?,
                        instance_id: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Persisted before the irreversible terminal write. The connection uses
    /// synchronous FULL, so returning success is the durability boundary.
    pub fn record_input_intent(&self, scope: &str, key: &str, intent: &InputIntent) -> Result<()> {
        self.connection.execute(
            "INSERT INTO input_intents(scope,key,fingerprint,operation_key,supervisor_id,instance_id) VALUES (?1,?2,?3,?4,?5,?6)",
            params![scope, key, intent.fingerprint, intent.operation_key, intent.supervisor_id, intent.instance_id],
        )?;
        Ok(())
    }

    pub fn input_intent_operation_pending(&self, operation_key: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM input_intents WHERE operation_key=?1)",
            [operation_key],
            |row| row.get(0),
        )?)
    }

    pub fn pane_launch<T: DeserializeOwned>(&self, pane_id: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM pane_launches WHERE pane_id=?1",
                [pane_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn has_pane_launch(&self, pane_id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pane_launches WHERE pane_id=?1)",
            [pane_id],
            |row| row.get(0),
        )?)
    }

    pub fn client_focus(&self, scope: &str) -> Result<Option<ClientFocus>> {
        Ok(self
            .connection
            .query_row(
                "SELECT workspace_id,pane_id FROM client_focus WHERE scope=?1",
                [scope],
                |row| {
                    Ok(ClientFocus {
                        workspace_id: row.get(0)?,
                        pane_id: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn idempotency_receipt(
        &self,
        scope: &str,
        key: &str,
    ) -> Result<Option<IdempotencyReceipt>> {
        let raw: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT fingerprint,result FROM idempotency WHERE scope=?1 AND key=?2",
                params![scope, key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        raw.map(|(fingerprint, result)| {
            Ok(IdempotencyReceipt {
                fingerprint,
                result: serde_json::from_str(&result)?,
            })
        })
        .transpose()
    }

    pub fn device_identity(&self, token_hash: &[u8]) -> Result<Option<DeviceIdentity>> {
        Ok(self
            .connection
            .query_row(
                "SELECT id,public_key FROM devices WHERE token_hash=?1 AND revoked=0",
                [token_hash],
                |row| {
                    Ok(DeviceIdentity {
                        device_id: row.get(0)?,
                        public_key: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn active_pair_requests<T: DeserializeOwned>(&self, now: i64) -> Result<Vec<T>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM pair_requests WHERE expires_unix>?1 ORDER BY id")?;
        let rows = statement.query_map([now], |row| row.get::<_, String>(0))?;
        rows.map(|raw| Ok(serde_json::from_str(&raw?)?)).collect()
    }

    pub fn active_pair_request<T: DeserializeOwned>(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<T>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM pair_requests WHERE id=?1 AND expires_unix>?2",
                params![id, now],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn active_pair_window_expiry(&self, code: &str, now: i64) -> Result<Option<i64>> {
        Ok(self
            .connection
            .query_row(
                "SELECT expires_unix FROM pair_windows WHERE code=?1 AND expires_unix>?2",
                params![code, now],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn pair_window_active(&self, code: &str, now: i64) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pair_windows WHERE code=?1 AND expires_unix>?2)",
            params![code, now],
            |row| row.get(0),
        )?)
    }

    pub fn device<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .connection
            .query_row("SELECT payload FROM devices WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }
}

impl Change<'_> {
    pub fn insert_workspace(&self, workspace: &Workspace) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO workspaces(id,payload) VALUES (?1,?2)",
            params![workspace.workspace_id, serde_json::to_string(workspace)?],
        )?;
        Ok(())
    }

    pub fn update_workspace(&self, workspace: &Workspace) -> Result<()> {
        self.transaction.execute(
            "UPDATE workspaces SET payload=?1 WHERE id=?2",
            params![serde_json::to_string(workspace)?, workspace.workspace_id],
        )?;
        Ok(())
    }

    pub fn focus_workspace(&self, scope: &str, workspace_id: &str) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO client_focus(scope,workspace_id,pane_id) VALUES (?1,?2,NULL) ON CONFLICT(scope) DO UPDATE SET workspace_id=excluded.workspace_id,pane_id=NULL",
            params![scope, workspace_id],
        )?;
        Ok(())
    }

    pub fn close_workspace(&self, workspace_id: &str) -> Result<()> {
        self.transaction
            .execute("UPDATE workspaces SET closed=1 WHERE id=?1", [workspace_id])?;
        self.transaction.execute(
            "UPDATE panes SET closed=1 WHERE workspace_id=?1",
            [workspace_id],
        )?;
        self.transaction.execute(
            "UPDATE client_focus SET workspace_id=NULL,pane_id=NULL WHERE workspace_id=?1",
            [workspace_id],
        )?;
        Ok(())
    }

    pub fn insert_pane(&self, pane: &PaneRecord) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO panes(id,workspace_id,payload,instance_key,sequence_base) VALUES (?1,?2,?3,?4,?5)",
            params![pane.pane.pane_id, pane.pane.workspace_id, serde_json::to_string(&pane.pane)?, pane.instance_key, pane.sequence_base.to_string()],
        )?;
        Ok(())
    }

    pub fn update_pane(&self, pane: &PaneRecord) -> Result<()> {
        self.transaction.execute(
            "UPDATE panes SET payload=?1,closed=?2,instance_key=?3,instance_id=?4,supervisor_id=?5,sequence_base=?6 WHERE id=?7",
            params![serde_json::to_string(&pane.pane)?, pane.closed, pane.instance_key, pane.instance_id, pane.supervisor_id, pane.sequence_base.to_string(), pane.pane.pane_id],
        )?;
        Ok(())
    }

    pub fn insert_pane_launch<T: Serialize>(&self, pane_id: &str, launch: &T) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO pane_launches(pane_id,payload) VALUES (?1,?2)",
            params![pane_id, serde_json::to_string(launch)?],
        )?;
        Ok(())
    }

    pub fn focus_pane(&self, scope: &str, workspace_id: &str, pane_id: &str) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO client_focus(scope,workspace_id,pane_id) VALUES (?1,?2,?3) ON CONFLICT(scope) DO UPDATE SET workspace_id=excluded.workspace_id,pane_id=excluded.pane_id",
            params![scope, workspace_id, pane_id],
        )?;
        Ok(())
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<()> {
        self.transaction
            .execute("UPDATE panes SET closed=1 WHERE id=?1", [pane_id])?;
        self.transaction.execute(
            "UPDATE client_focus SET pane_id=NULL WHERE pane_id=?1",
            [pane_id],
        )?;
        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &Value) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value.to_string()],
        )?;
        Ok(())
    }

    pub fn remove_input_intent(&self, scope: &str, key: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM input_intents WHERE scope=?1 AND key=?2",
            params![scope, key],
        )?;
        Ok(())
    }

    pub fn update_idempotency_result(
        &self,
        scope: &str,
        key: &str,
        result: &Value,
    ) -> Result<bool> {
        Ok(self.transaction.execute(
            "UPDATE idempotency SET result=?1 WHERE scope=?2 AND key=?3",
            params![result.to_string(), scope, key],
        )? == 1)
    }

    pub fn retire_agent_outbox_process(
        &self,
        pane_id: &str,
        instance_id: &str,
        supervisor_id: &str,
    ) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM agent_outbox WHERE pane_id=?1 AND instance_id=?2 AND supervisor_id=?3",
            params![pane_id, instance_id, supervisor_id],
        )?;
        Ok(())
    }

    pub fn clear_attention(&self, pane_id: &str) -> Result<()> {
        self.transaction
            .execute("DELETE FROM agent_attention WHERE pane_id=?1", [pane_id])?;
        Ok(())
    }

    pub fn remove_attention(&self, pane_id: &str, attention_id: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM agent_attention WHERE pane_id=?1 AND attention_id=?2",
            params![pane_id, attention_id],
        )?;
        Ok(())
    }

    pub fn clear_failed_probes(&self, fingerprint: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM agent_probes WHERE fingerprint=?1 AND json_extract(payload,'$.result') IN ('unauthenticated','inconclusive')",
            [fingerprint],
        )?;
        Ok(())
    }

    pub fn insert_pair_request<T: Serialize>(
        &self,
        id: &str,
        value: &T,
        expires: i64,
        now: i64,
    ) -> Result<()> {
        let count: i64 = self.transaction.query_row(
            "SELECT COUNT(*) FROM pair_requests WHERE expires_unix>?1",
            [now],
            |row| row.get(0),
        )?;
        if count >= 64 {
            return Err(Error::Invalid("pairing request limit reached"));
        }
        self.transaction.execute(
            "INSERT INTO pair_requests(id,payload,expires_unix) VALUES (?1,?2,?3)",
            params![id, serde_json::to_string(value)?, expires],
        )?;
        Ok(())
    }

    pub fn update_pair_request<T: Serialize>(&self, id: &str, value: &T) -> Result<()> {
        self.transaction.execute(
            "UPDATE pair_requests SET payload=?1 WHERE id=?2",
            params![serde_json::to_string(value)?, id],
        )?;
        Ok(())
    }

    pub fn open_pair_window(&self, code: &str, expires: i64, now: i64) -> Result<()> {
        self.transaction
            .execute("DELETE FROM pair_windows WHERE expires_unix<=?1", [now])?;
        self.transaction
            .execute("DELETE FROM pair_requests WHERE expires_unix<=?1", [now])?;
        self.transaction.execute(
            "INSERT INTO pair_windows(code,expires_unix) VALUES (?1,?2)",
            params![code, expires],
        )?;
        Ok(())
    }

    pub fn revoke_device<T: Serialize>(&self, id: &str, device: &T) -> Result<()> {
        self.transaction.execute(
            "UPDATE devices SET payload=?1,revoked=1 WHERE id=?2",
            params![serde_json::to_string(device)?, id],
        )?;
        Ok(())
    }

    pub fn consume_pair_window_and_insert_device<T: Serialize>(
        &self,
        code: &str,
        device_id: &str,
        device: &T,
        public_key: &[u8],
        token_hash: &[u8],
    ) -> Result<()> {
        self.transaction
            .execute("DELETE FROM pair_windows WHERE code=?1", [code])?;
        self.transaction.execute(
            "INSERT INTO devices(id,payload,public_key,token_hash) VALUES (?1,?2,?3,?4)",
            params![
                device_id,
                serde_json::to_string(device)?,
                public_key,
                token_hash
            ],
        )?;
        Ok(())
    }
}
