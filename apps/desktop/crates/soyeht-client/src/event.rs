use crate::{snapshot::Snapshot, streams::StreamOutput};
use serde_json::Value;
use soyeht_protocol::{EventName, HarnessInfo, TranscriptMessage, UsageReading};

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
    /// An agent pane's transcript, as the engine holds it.
    Transcript {
        pane_id: String,
        items: Vec<TranscriptMessage>,
    },
    /// The pane a request to open one (`Command::CreateTerminal`, `CreateAgent`) created.
    Opened { ticket: u64, pane_id: String },
    /// The request to open a pane that `ticket` names failed.
    OpenFailed { ticket: u64 },
    /// The usage of every provider.
    Usage(Vec<UsageReading>),
    /// The agent harnesses the engine can start.
    Harnesses(Vec<HarnessInfo>),
}
