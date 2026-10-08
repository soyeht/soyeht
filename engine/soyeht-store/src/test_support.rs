//! Narrow database controls used by cross-crate integration tests.
//!
//! This feature keeps driver access and query text inside the persistence
//! crate while letting process-level tests inspect durable boundaries.

use crate::database::{Error, Result};
use rusqlite::{
    Connection, OptionalExtension, params,
    trace::{TraceEvent, TraceEventCodes},
};
use serde_json::Value;
use std::{cell::Cell, path::Path, time::Duration};

thread_local! {
    static STATEMENTS: Cell<u64> = const { Cell::new(0) };
}

/// Counts every statement the engine's connection starts, per thread, so a
/// test can assert that a code path runs no SQL.
pub(crate) fn count_statements(connection: &Connection) {
    connection.trace_v2(
        TraceEventCodes::SQLITE_TRACE_STMT,
        Some(|event| {
            if matches!(event, TraceEvent::Stmt(..)) {
                STATEMENTS.with(|count| count.set(count.get() + 1));
            }
        }),
    );
}

/// Statements started by stores opened on this thread, since the thread began.
pub fn statements_started() -> u64 {
    STATEMENTS.with(Cell::get)
}

pub struct DatabaseProbe {
    connection: Connection,
}

impl DatabaseProbe {
    pub fn open(directory: &Path) -> Result<Self> {
        let connection = Connection::open(directory.join("soyeht.db"))?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(Self { connection })
    }

    pub fn age_agent(&self, pane_id: &str, cancel: bool) -> Result<()> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM agent_runtime WHERE pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let mut state: Value = serde_json::from_str(&raw)?;
        state["last_update_ms"] = Value::from(0);
        if state["active_silent_since_ms"].is_i64() {
            state["active_silent_since_ms"] = Value::from(0);
        }
        let pending = state["client"]["pending"]
            .as_object_mut()
            .ok_or(Error::TestFixture("test runtime has no pending calls"))?;
        for call in pending.values_mut() {
            call["sent_at_ms"] = Value::from(0);
        }
        if cancel {
            if !state["cancel_at_ms"].is_i64() {
                return Err(Error::TestFixture("test runtime has no cancellation"));
            }
            state["cancel_at_ms"] = Value::from(0);
        }
        self.connection.execute(
            "UPDATE agent_runtime SET payload=?1 WHERE pane_id=?2",
            params![state.to_string(), pane_id],
        )?;
        Ok(())
    }

    pub fn set_agent_silent_since(&self, pane_id: &str, value: i64) -> Result<()> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM agent_runtime WHERE pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let mut state: Value = serde_json::from_str(&raw)?;
        if !state["active_silent_since_ms"].is_i64() {
            return Err(Error::TestFixture("test agent has no silent prompt"));
        }
        state["active_silent_since_ms"] = Value::from(value);
        self.connection.execute(
            "UPDATE agent_runtime SET payload=?1 WHERE pane_id=?2",
            params![state.to_string(), pane_id],
        )?;
        Ok(())
    }

    pub fn set_experiment_capture(&self, pane_id: &str, on: bool) -> Result<()> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM panes WHERE id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let mut pane: Value = serde_json::from_str(&raw)?;
        pane["experimentCapture"] = serde_json::json!({"on": on});
        self.connection.execute(
            "UPDATE panes SET payload=?1 WHERE id=?2",
            params![pane.to_string(), pane_id],
        )?;
        Ok(())
    }

    pub fn install_terminal_harness_session(
        &self,
        pane_id: &str,
        command: &Path,
        capability: &str,
    ) -> Result<u32> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM panes WHERE id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let instance_key = format!("managed-terminal-{capability}");
        let mut pane: Value = serde_json::from_str(&raw)?;
        let generation = pane["generation"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(Error::TestFixture("test terminal has no generation"))?;
        let cwd = pane["cwd"]
            .as_str()
            .map(str::to_owned)
            .ok_or(Error::TestFixture("test terminal has no directory"))?;
        let cols = pane["cols"]
            .as_u64()
            .ok_or(Error::TestFixture("test terminal has no columns"))?;
        let rows = pane["rows"]
            .as_u64()
            .ok_or(Error::TestFixture("test terminal has no rows"))?;
        pane["pid"] = Value::Null;
        pane["state"] = Value::from("starting");
        pane["phrase"] = Value::from("Starting terminal harness");
        pane["experimentCapture"] = serde_json::json!({"on": true});
        let launch = serde_json::json!({
            "launchType":"terminal_harness",
            "spawn":{
                "instance_key":instance_key,
                "generation":generation,
                "kind":"pty",
                "cwd":cwd,
                "argv":[command,"--settings","{}"],
                "env":{},
                "cols":cols,
                "rows":rows
            },
            "harness":{"identity":"known","id":"registry.claude-acp","version":null},
            "hookBinding":capability
        });
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "UPDATE panes SET payload=?1,instance_key=?2,instance_id=NULL,supervisor_id=NULL WHERE id=?3",
            params![pane.to_string(), instance_key, pane_id],
        )?;
        transaction.execute(
            "INSERT INTO pane_launches(pane_id,payload) VALUES (?1,?2) ON CONFLICT(pane_id) DO UPDATE SET payload=excluded.payload",
            params![pane_id, launch.to_string()],
        )?;
        transaction.commit()?;
        Ok(generation)
    }

    pub fn agent_outbox_empty(&self, pane_id: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM agent_outbox WHERE pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        Ok(count == 0)
    }

    pub fn agent_silence(&self, pane_id: &str) -> Result<(bool, Option<i64>)> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM agent_runtime WHERE pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let state: Value = serde_json::from_str(&raw)?;
        Ok((
            state["active_output_observed"].as_bool().unwrap_or(true),
            state["active_silent_since_ms"].as_i64(),
        ))
    }

    pub fn editor_draft(&self, file_id: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT draft_text FROM editor_files WHERE file_id=?1",
                [file_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn make_task_retry_due(&self, workspace_id: &str, task_id: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE task_block_retries SET due_ms=0 WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, task_id],
        )?;
        if changed != 1 {
            return Err(Error::TestFixture("test task has no scheduled retry"));
        }
        Ok(())
    }

    pub fn agent_binding(&self, pane_id: &str) -> Result<String> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM agent_runtime WHERE pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let state: Value = serde_json::from_str(&raw)?;
        state["binding"]
            .as_str()
            .map(str::to_owned)
            .ok_or(Error::TestFixture("test runtime has no agent binding"))
    }

    pub fn remove_agent_binding(&self, pane_id: &str) -> Result<()> {
        let raw: String = self.connection.query_row(
            "SELECT payload FROM agent_runtime WHERE pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        let mut state: Value = serde_json::from_str(&raw)?;
        state
            .as_object_mut()
            .ok_or(Error::TestFixture("test runtime is not an object"))?
            .remove("binding")
            .ok_or(Error::TestFixture("test runtime has no agent binding"))?;
        self.connection.execute(
            "UPDATE agent_runtime SET payload=?1 WHERE pane_id=?2",
            params![state.to_string(), pane_id],
        )?;
        Ok(())
    }

    pub fn event_count(&self, method: &str) -> Result<u64> {
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM events WHERE method=?1",
            [method],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| Error::TestFixture("negative test event count"))
    }

    pub fn event_count_by_field(&self, method: &str, field: &str, value: &str) -> Result<u64> {
        let path = format!("$.{field}");
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM events WHERE method=?1 AND json_extract(params,?2)=?3",
            params![method, path, value],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| Error::TestFixture("negative test event count"))
    }

    pub fn latest_event(&self, method: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT params FROM events WHERE method=?1 ORDER BY id DESC LIMIT 1",
                [method],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn request_result(&self, method: &str, pane_id: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT result FROM agent_requests WHERE method=?1 AND pane_id=?2",
                params![method, pane_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn latest_request_result(&self, method: &str, pane_id: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT result FROM agent_requests WHERE method=?1 AND pane_id=?2 ORDER BY source_seq DESC LIMIT 1",
                params![method, pane_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn open_request_count(&self, pane_id: &str) -> Result<u64> {
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM agent_requests WHERE result IS NULL AND pane_id=?1",
            [pane_id],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| Error::TestFixture("negative test request count"))
    }

    pub fn request_count(&self, method: &str) -> Result<u64> {
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM agent_requests WHERE method=?1",
            [method],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| Error::TestFixture("negative test request count"))
    }

    pub fn engine_generation(&self) -> Result<u64> {
        let value: String = self.connection.query_row(
            "SELECT value FROM metadata WHERE key='engine_generation'",
            [],
            |row| row.get(0),
        )?;
        value
            .parse()
            .map_err(|_| Error::TestFixture("invalid test engine generation"))
    }

    pub fn expire_request(&self, request_id: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE agent_requests SET expires_ms=0 WHERE request_id=?1",
            [request_id],
        )?;
        if changed != 1 {
            return Err(Error::TestFixture("test request not found"));
        }
        Ok(())
    }
}
