use crate::{
    errors::{self, Result},
    outbox::Outbox,
    ports::StorePort,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{Actor, ErrorKind, Workspace, frame::Frame};
use std::collections::BTreeMap;

pub use soyeht_store::domain::PaneRecord as StoredPane;
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

pub fn notification(method: &str, params: Value) -> Result<Frame> {
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","method":method,"params":params}),
    )?))
}
pub fn response(id: Value, result: Value) -> Result<Frame> {
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","id":id,"result":result}),
    )?))
}
pub fn failure(id: Value, error: soyeht_protocol::RpcError) -> Result<Frame> {
    Ok(Frame::Json(errors::json_bytes(
        &json!({"jsonrpc":"2.0","id":id,"error":error}),
    )?))
}

pub fn workspace(store: &impl StorePort, id: &str) -> Result<Workspace> {
    store
        .workspace(id)
        .map_err(errors::storage)?
        .ok_or_else(|| errors::error(ErrorKind::NotFound, "Workspace not found"))
}

pub fn panes(store: &impl StorePort) -> Result<Vec<StoredPane>> {
    store.panes().map_err(errors::storage)
}
