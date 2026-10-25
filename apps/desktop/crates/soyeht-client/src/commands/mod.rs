//! What the user can ask of the engine, and the typed protocol requests that carry it out.

mod agents;
mod coalesce;
mod outcome;
mod panes;
mod tasks;
mod workspaces;

use crate::{
    attachments::{Draft, Upload},
    editor::EditorCommand,
    event::Event,
    preference_requests,
    writing::Command as WritingCommand,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use soyeht_protocol::{
    AgentPaneRole, LoopStopWhen, PaneInputParams, Request, TaskExecutor, TaskPriority, TaskState,
    UsageProvider, WorkspacePaneLayoutEnvelope, client_policy,
};
use uuid::Uuid;

pub use coalesce::coalesce;
pub use outcome::outcome;

/// Raw input goes out in pieces of the contract's size, as on the Mac client.
pub const INPUT_CHUNK_BYTES: usize = client_policy::TERMINAL_INPUT_CHUNK_BYTES;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Input {
        pane_id: String,
        bytes: Vec<u8>,
    },
    Resize {
        pane_id: String,
        rows: u16,
        cols: u16,
    },
    Subscribe {
        pane_id: String,
    },
    Focus {
        pane_id: String,
    },
    CreateTerminal {
        workspace_id: String,
        /// The folder the shell starts in; `None` is the workspace's folder.
        cwd: Option<String>,
        /// Names this request in the answer (`Event::Opened`, `Event::OpenFailed`).
        ticket: u64,
    },
    ClosePane {
        pane_id: String,
    },
    /// A new agent pane running `harness`, started with the harness's own defaults.
    CreateAgent {
        workspace_id: String,
        harness: String,
        /// The folder the agent works in; `None` is the workspace's folder.
        cwd: Option<String>,
        ticket: u64,
    },
    /// Make a workspace of the folder at `path`. The answer (`Event::WorkspaceOpened`) carries
    /// the ticket.
    CreateWorkspace {
        name: String,
        path: String,
        ticket: u64,
    },
    RenameWorkspace {
        workspace_id: String,
        name: String,
    },
    RenamePane {
        pane_id: String,
        name: String,
    },
    /// Send the person's message, and the files attached to it, to an agent.
    Prompt {
        pane_id: String,
        text: String,
        attachments: Vec<Draft>,
    },
    /// Stop the running turn.
    Cancel {
        pane_id: String,
    },
    LoadTranscript {
        pane_id: String,
    },
    /// The page of an agent's transcript before the one starting at `before`.
    LoadOlder {
        pane_id: String,
        before: String,
    },
    /// The messages waiting for an agent, all of their pages.
    LoadQueue {
        pane_id: String,
    },
    LoadUsage,
    /// Ask the usage readers for a fresh reading, of one provider or of all.
    RefreshUsage {
        provider: Option<UsageProvider>,
    },
    /// Fetch the agent harnesses the engine can start. `rescan` reads the login-shell environment
    /// again first.
    LoadHarnesses {
        rescan: bool,
    },
    /// Fetch the models and options a harness offers before a pane runs it.
    LoadCatalogue {
        harness: String,
    },
    /// Fetch the settings Preferences shows: the default harness and the default permission mode
    /// of each of `harnesses`.
    LoadPreferences {
        harnesses: Vec<String>,
    },
    /// Fetch the skills every harness loads: the user's with `None`, or the user's and those of
    /// the workspace.
    LoadSkills {
        workspace_id: Option<String>,
    },
    /// Fetch the commands the pane's harness advertised, for the composer's "/" list.
    LoadCommands {
        pane_id: String,
    },
    /// Ask the engine which of the programs the app needs this machine has.
    LoadReadiness,
    /// Ask the engine whether it has its link to the supervisor of the terminals.
    LoadSupervisor,
    /// Make a skill of the user's: its folder with a `SKILL.md`.
    CreateSkill {
        name: String,
        description: String,
        instructions: String,
    },
    DeleteSkill {
        name: String,
    },
    /// Make `harness` the one new agents start with.
    SetPreferredHarness {
        harness: String,
    },
    /// The permission mode new panes of `harness` start in; `None` keeps the harness's own.
    SetDefaultMode {
        harness: String,
        mode: Option<String>,
    },
    /// Ask the agent to forget its account; it then asks for a sign-in again.
    SignOut {
        pane_id: String,
    },
    /// Choose a value of one of the pane's agent options (model, effort, permission mode).
    SetConfig {
        pane_id: String,
        config_id: String,
        value: String,
    },
    /// Restart the pane on `harness` and hand the conversation over. No model or effort leaves
    /// the choice to the harness.
    SwitchHarness {
        pane_id: String,
        harness: String,
        model: Option<String>,
        effort: Option<String>,
    },
    AnswerPermission {
        pane_id: String,
        request_id: String,
        option_id: String,
    },
    /// All the tasks of a workspace, every page of them.
    LoadTasks {
        workspace_id: String,
    },
    /// One task with its subitems, comments, attempts and history.
    LoadTask {
        workspace_id: String,
        task_id: String,
    },
    CreateTask {
        task: Box<NewTask>,
        /// Names the pane the request opens when it dispatches (`Event::TaskPane`).
        ticket: u64,
    },
    MoveTask {
        workspace_id: String,
        task_id: String,
        state: TaskState,
        reason: String,
    },
    /// Hand a task to an agent in a new pane.
    DispatchTask {
        workspace_id: String,
        task_id: String,
        preset: TaskExecutor,
        ticket: u64,
    },
    /// Start, change or stop the pane's loop.
    SetLoop {
        pane_id: String,
        prompt: String,
        interval: u32,
        max_rounds: u32,
        stop_when: LoopStopWhen,
        enabled: bool,
    },
    SetRole {
        pane_id: String,
        role: AgentPaneRole,
    },
    /// Tell the engine the person has a draft in the pane's composer (or has not any more), so
    /// that a loop never sends over it.
    SetDraft {
        pane_id: String,
        active: bool,
    },
    /// Take a message out of the queue of messages waiting for an agent.
    RemoveQueued {
        pane_id: String,
        prompt_id: String,
    },
    ResumeQueue {
        pane_id: String,
    },
    /// Answer the agent's question with the fields the person filled in, or decline it.
    AnswerElicitation {
        pane_id: String,
        request_id: String,
        fields: Option<Value>,
        cancel: bool,
    },
    /// Start signing in with one of the methods the agent offers.
    StartSignIn {
        pane_id: String,
        method_id: Option<String>,
    },
    CancelSignIn {
        pane_id: String,
    },
    SetLayout {
        workspace_id: String,
        layout: WorkspacePaneLayoutEnvelope,
    },
    CloseWorkspace {
        workspace_id: String,
    },
    /// What the person does in an editor pane.
    Editor(EditorCommand),
    /// What a writing pane and the lists of the sections ask of the engine.
    Writing(WritingCommand),
}

/// What the new-task form asks the engine to create.
#[derive(Debug, Clone, PartialEq)]
pub struct NewTask {
    pub workspace_id: String,
    pub title: String,
    pub description: String,
    pub how_to_verify: Option<String>,
    pub must_not_change: String,
    pub executor: Option<TaskExecutor>,
    pub priority: TaskPriority,
    pub create_worktree: bool,
    pub dependencies: Vec<String>,
    /// Dispatch it to an agent as soon as it exists.
    pub dispatch: bool,
}

pub(crate) fn key() -> String {
    Uuid::new_v4().to_string()
}

pub fn input_request(pane_id: &str, bytes: &[u8]) -> Request {
    Request::PaneInput(PaneInputParams {
        pane_id: pane_id.to_owned(),
        bytes: STANDARD.encode(bytes),
        idempotency_key: key(),
    })
}

/// The single request a command needs. Input is sent in pieces by the caller.
pub fn request(command: &Command) -> Option<Request> {
    preference_requests::request(command)
        .or_else(|| panes::request(command))
        .or_else(|| workspaces::request(command))
        .or_else(|| tasks::request(command))
        .or_else(|| agents::request(command))
}

/// The bytes a command sends as binary frames before its request.
pub fn uploads(command: &Command) -> Vec<Upload> {
    match command {
        Command::Prompt { attachments, .. } => attachments
            .iter()
            .filter_map(|draft| draft.wire().1)
            .collect(),
        _ => Vec::new(),
    }
}

impl Command {
    /// The request that opens a pane names it, so the answer can be matched to it.
    pub fn ticket(&self) -> Option<u64> {
        match self {
            Self::CreateTerminal { ticket, .. }
            | Self::CreateAgent { ticket, .. }
            | Self::CreateWorkspace { ticket, .. }
            | Self::CreateTask { ticket, .. }
            | Self::DispatchTask { ticket, .. } => Some(*ticket),
            _ => None,
        }
    }

    /// What the application learns when this command fails for `error`, so that it stops waiting
    /// for the answer.
    pub fn failure(&self, error: &crate::Error) -> Option<Event> {
        match self {
            Self::Editor(editor) => crate::editor::commands::refused(editor, error)
                .or_else(|| crate::editor::commands::failure(editor, &error.to_string())),
            Self::Writing(writing) => writing.failure(),
            // The arrangement on screen goes back to the stored one, and the person is told.
            Self::SetLayout { workspace_id, .. } => Some(Event::LayoutRejected {
                workspace_id: workspace_id.clone(),
            }),
            _ => None,
        }
    }

    /// Whether `error` is shown where it happened, so that the problem banner shows nothing of
    /// its own: in the pane of a file the engine refused as text, or by the command's own
    /// failure event (a layout the engine refused says so in its message).
    pub fn failure_is_shown_in_place(&self, error: &crate::Error) -> bool {
        matches!(self, Self::SetLayout { .. })
            || matches!(self, Self::Writing(writing) if writing.failure_is_shown_in_place())
            || matches!(self, Self::Editor(editor) if crate::editor::commands::refused(editor, error).is_some())
    }

    /// Fetches that the interface does not wait on. They run apart from the commands that carry
    /// the person's typing, so a burst of them never delays a keystroke.
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            Self::LoadTranscript { .. }
                | Self::LoadTasks { .. }
                | Self::LoadTask { .. }
                | Self::LoadOlder { .. }
                | Self::LoadQueue { .. }
                | Self::LoadUsage
                | Self::LoadHarnesses { .. }
                | Self::LoadCatalogue { .. }
                | Self::LoadPreferences { .. }
                | Self::LoadSkills { .. }
                | Self::LoadCommands { .. }
                | Self::LoadReadiness
                | Self::LoadSupervisor
        ) || matches!(self, Self::Editor(editor) if editor.is_background())
            || matches!(self, Self::Writing(writing) if writing.is_background())
    }
}

#[cfg(test)]
mod tests;
