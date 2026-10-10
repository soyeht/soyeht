use crate::{snapshot::Snapshot, streams::StreamOutput};
use serde_json::Value;
use soyeht_protocol::EventName;

/// What the connection tells the application, in the order it happened.
#[derive(Debug, Clone)]
pub enum Event {
    /// A connection is established; this is the whole engine state to start from.
    Connected(Snapshot),
    /// The connection ended. The supervisor is already reconnecting.
    Disconnected(String),
    /// A notification the engine pushed after the snapshot.
    Notification { name: EventName, params: Value },
    /// Output of a subscribed terminal.
    Terminal(StreamOutput),
    /// A request the user asked for failed.
    Problem(String),
}
