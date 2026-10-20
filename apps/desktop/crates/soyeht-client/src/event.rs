use crate::{
    catalogue::Catalogue, editor::EditorEvent, preferences::Preferences, snapshot::Snapshot,
    streams::StreamOutput, tasks::TaskUpdate,
};
use serde_json::Value;
use soyeht_protocol::{
    EventName, HarnessInfo, LoopConfig, PromptItem, SkillsListResult, Task, TranscriptMessage,
    UsageReading, Workspace,
};

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
        /// Where the page before this one starts, when the engine has earlier messages.
        next_cursor: Option<String>,
    },
    /// What the engine told about the tasks.
    Task(TaskUpdate),
    /// A task a request created or dispatched, with the pane it opened for it, if any.
    TaskPane {
        task: Task,
        ticket: u64,
        pane_id: Option<String>,
    },
    /// The loop the engine accepted for a pane.
    LoopSet { pane_id: String, config: LoopConfig },
    /// The messages waiting for an agent pane, as the engine holds them.
    Queue {
        pane_id: String,
        items: Vec<PromptItem>,
        paused: bool,
    },
    /// The pane a request to open one (`Command::CreateTerminal`, `CreateAgent`) created.
    Opened { ticket: u64, pane_id: String },
    /// The workspace a request to open a folder (`Command::CreateWorkspace`) made.
    WorkspaceOpened { ticket: u64, workspace: Workspace },
    /// The request to open a pane that `ticket` names failed.
    OpenFailed { ticket: u64 },
    /// The usage of every provider.
    Usage(Vec<UsageReading>),
    /// The agent harnesses the engine can start.
    Harnesses(Vec<HarnessInfo>),
    /// The models and options a harness offers.
    Catalogue {
        harness: String,
        catalogue: Catalogue,
    },
    /// The models of a harness could not be read.
    CatalogueFailed { harness: String, reason: String },
    /// The skills every harness loads.
    Skills(SkillsListResult),
    /// The settings Preferences shows.
    Preferences(Preferences),
    /// What the engine told an editor pane.
    Editor(EditorEvent),
}
