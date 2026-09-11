use crate::{
    errors::{self, Result},
    outbox::Outbox,
    screen::Screen,
};
use rusqlite::{OptionalExtension, Row};
use serde_json::{Value, json};
use soyeht_protocol::{Actor, ErrorKind, Pane, Workspace, frame::Frame};
use soyeht_store::database::Store;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct StoredPane {
    pub pane: Pane,
    pub closed: bool,
    pub instance_key: String,
    pub instance_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub sequence_base: u64,
}
pub struct RunningPane {
    pub stored: StoredPane,
    pub screen: Screen,
}

pub struct Subscription {
    pub binary_ref: u32,
    pub stream_id: String,
    pub generation: u32,
    pub next: u64,
    pub replay_until: u64,
    pub replaying: bool,
    pub current: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.current
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

pub struct Connected {
    pub outbox: Outbox,
    pub actor: Option<Actor>,
    pub scope: String,
    pub local: bool,
    pub nonce: [u8; 32],
    pub events: bool,
    pub streams: BTreeMap<String, Subscription>,
    pub next_binary_ref: u32,
    pub pair_request: Option<String>,
}

pub fn notification(method: &str, params: Value) -> Frame {
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","method":method,"params":params}))
            .expect("JSON notification"),
    )
}
pub fn response(id: Value, result: Value) -> Frame {
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result}))
            .expect("JSON response"),
    )
}
pub fn failure(id: Value, error: soyeht_protocol::RpcError) -> Frame {
    Frame::Json(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"error":error})).expect("JSON error"),
    )
}

pub fn workspace(store: &Store, id: &str) -> Result<Workspace> {
    let raw: Option<String> = store
        .connection()
        .query_row(
            "SELECT payload FROM workspaces WHERE id=?1 AND closed=0",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| errors::storage(e.into()))?;
    serde_json::from_str(
        &raw.ok_or_else(|| errors::error(ErrorKind::NotFound, "Workspace not found"))?,
    )
    .map_err(|_| errors::error(ErrorKind::InternalError, "Invalid workspace record"))
}

fn pane_row(row: &Row<'_>) -> rusqlite::Result<StoredPane> {
    let payload: String = row.get(0)?;
    let pane = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let base: String = row.get(5)?;
    let sequence_base = base.parse().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(StoredPane {
        pane,
        closed: row.get(1)?,
        instance_key: row.get(2)?,
        instance_id: row.get(3)?,
        supervisor_id: row.get(4)?,
        sequence_base,
    })
}

pub fn panes(store: &Store) -> Result<Vec<StoredPane>> {
    let mut statement = store.connection().prepare("SELECT payload,closed,instance_key,instance_id,supervisor_id,sequence_base FROM panes ORDER BY rowid").map_err(|e|errors::storage(e.into()))?;
    statement
        .query_map([], pane_row)
        .map_err(|e| errors::storage(e.into()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| errors::storage(e.into()))
}
