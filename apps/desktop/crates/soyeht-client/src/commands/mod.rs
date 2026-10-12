//! What the user can ask of the engine, and the typed protocol requests that carry it out.

use crate::tasks::TaskUpdate;
use crate::{
    Error, Result,
    agent::TRANSCRIPT_PAGE,
    attachments::{Draft, Upload},
    event::Event,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use soyeht_protocol::{
    AgentPaneRole, ConfigValue, HarnessInfo, HarnessListParams, HarnessListResult, LoopSetParams,
    LoopSetResult, LoopStopWhen, PaneAuthCancelParams, PaneAuthStartParams, PaneCancelParams,
    PaneCloseParams, PaneConfigSetParams, PaneCreateParams, PaneCreateResult, PaneDraftParams,
    PaneElicitationAnswerParams, PaneFocusParams, PaneHarnessSwitchParams, PaneInputParams,
    PaneMode, PanePermissionAnswerParams, PanePromptParams, PaneQueueRemoveParams,
    PaneQueueResumeParams, PaneResizeParams, PaneRoleSetParams, PaneStreamSubscribeParams,
    PaneTranscriptParams, PaneTranscriptResult, PromptOrigin, ReplayFrom, Request, TailSelector,
    TaskCreateParams, TaskCreateResult, TaskDispatchParams, TaskDispatchResult, TaskExecutor,
    TaskMoveParams, TaskMoveResult, TaskPriority, TaskShowParams, TaskShowResult, TaskState,
    UsageListParams, UsageListResult, WorkspaceCloseParams, WorkspaceLayoutSetParams,
    WorkspacePaneLayoutEnvelope,
};
use std::sync::mpsc;
use uuid::Uuid;

/// Raw input goes out in pieces of this size, as on the Mac client.
pub const INPUT_CHUNK_BYTES: usize = 16 * 1024;

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
        ticket: u64,
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
    LoadHarnesses,
    /// Choose a value of one of the pane's agent options (model, effort, permission mode).
    SetConfig {
        pane_id: String,
        config_id: String,
        value: String,
    },
    SwitchHarness {
        pane_id: String,
        harness: String,
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

fn key() -> String {
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
    Some(match command {
        Command::Input { .. } => return None,
        Command::Resize {
            pane_id,
            rows,
            cols,
        } => Request::PaneResize(PaneResizeParams {
            pane_id: pane_id.clone(),
            cols: u32::from(*cols),
            rows: u32::from(*rows),
            idempotency_key: key(),
        }),
        Command::Subscribe { pane_id } => Request::PaneStreamSubscribe(PaneStreamSubscribeParams {
            pane_id: pane_id.clone(),
            from: ReplayFrom::V0(TailSelector::Tail),
        }),
        Command::Focus { pane_id } => Request::PaneFocus(PaneFocusParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::CreateTerminal { workspace_id, .. } => {
            create(workspace_id, PaneMode::Terminal, None)
        }
        Command::CreateAgent {
            workspace_id,
            harness,
            ..
        } => create(workspace_id, PaneMode::Agent, Some(harness)),
        Command::ClosePane { pane_id } => Request::PaneClose(PaneCloseParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::Prompt {
            pane_id,
            text,
            attachments,
        } => Request::PanePrompt(PanePromptParams {
            pane_id: pane_id.clone(),
            text: text.clone(),
            origin: Some(PromptOrigin::Human),
            attachments: (!attachments.is_empty())
                .then(|| attachments.iter().map(|draft| draft.wire().0).collect()),
            idempotency_key: key(),
        }),
        Command::Cancel { pane_id } => Request::PaneCancel(PaneCancelParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::LoadTranscript { pane_id } => transcript_page(pane_id, None),
        Command::LoadOlder { pane_id, before } => transcript_page(pane_id, Some(before)),
        // The queue comes in pages: the engine fetches them one after the other.
        Command::LoadQueue { .. } => return None,
        Command::LoadUsage => Request::UsageList(UsageListParams {}),
        Command::LoadHarnesses => Request::HarnessList(HarnessListParams {
            cursor: None,
            limit: None,
            rescan: None,
        }),
        Command::SetConfig {
            pane_id,
            config_id,
            value,
        } => Request::PaneConfigSet(PaneConfigSetParams {
            pane_id: pane_id.clone(),
            config_id: config_id.clone(),
            value: ConfigValue::V0(value.clone()),
            idempotency_key: key(),
        }),
        Command::SwitchHarness { pane_id, harness } => {
            Request::PaneHarnessSwitch(PaneHarnessSwitchParams {
                pane_id: pane_id.clone(),
                harness: harness.clone(),
                model: None,
                effort: None,
                idempotency_key: key(),
            })
        }
        Command::AnswerPermission {
            pane_id,
            request_id,
            option_id,
        } => Request::PanePermissionAnswer(PanePermissionAnswerParams {
            pane_id: pane_id.clone(),
            request_id: request_id.clone(),
            option_id: option_id.clone(),
            idempotency_key: key(),
        }),
        Command::LoadTasks { .. } => return None,
        Command::SetLayout {
            workspace_id,
            layout,
        } => Request::WorkspaceLayoutSet(WorkspaceLayoutSetParams {
            workspace_id: workspace_id.clone(),
            layout: layout.clone(),
            idempotency_key: key(),
        }),
        Command::CloseWorkspace { workspace_id } => Request::WorkspaceClose(WorkspaceCloseParams {
            workspace_id: workspace_id.clone(),
            idempotency_key: key(),
        }),
        other => return follow_up_request(other),
    })
}

/// The requests of what the person does after a pane or a task exists: reading and moving
/// tasks, loops, roles, drafts, the queue, questions and sign-ins.
fn follow_up_request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::LoadTask {
            workspace_id,
            task_id,
        } => Request::TaskShow(TaskShowParams {
            workspace_id: workspace_id.clone(),
            task_id: task_id.clone(),
        }),
        Command::CreateTask { task, .. } => Request::TaskCreate(TaskCreateParams {
            workspace_id: task.workspace_id.clone(),
            title: task.title.clone(),
            description: Some(task.description.clone()),
            how_to_verify: task.how_to_verify.clone(),
            must_not_change: task.must_not_change.clone(),
            executor: task.executor.clone(),
            priority: Some(task.priority.clone()),
            create_worktree: Some(task.create_worktree),
            permission_mode: None,
            dependencies: Some(task.dependencies.clone()),
            dispatch: Some(task.dispatch),
            idempotency_key: key(),
        }),
        Command::MoveTask {
            workspace_id,
            task_id,
            state,
            reason,
        } => Request::TaskMove(TaskMoveParams {
            workspace_id: workspace_id.clone(),
            task_id: task_id.clone(),
            state: state.clone(),
            reason: reason.clone(),
            idempotency_key: key(),
        }),
        Command::DispatchTask {
            workspace_id,
            task_id,
            preset,
            ..
        } => Request::TaskDispatch(TaskDispatchParams {
            workspace_id: workspace_id.clone(),
            task_id: task_id.clone(),
            preset: preset.clone(),
            idempotency_key: key(),
        }),
        Command::SetLoop {
            pane_id,
            prompt,
            interval,
            max_rounds,
            stop_when,
            enabled,
        } => Request::LoopSet(LoopSetParams {
            pane_id: pane_id.clone(),
            prompt: prompt.clone(),
            interval: *interval,
            max_rounds: *max_rounds,
            stop_when: stop_when.clone(),
            enabled: *enabled,
            idempotency_key: key(),
        }),
        Command::SetRole { pane_id, role } => Request::PaneRoleSet(PaneRoleSetParams {
            pane_id: pane_id.clone(),
            role: role.clone(),
            idempotency_key: key(),
        }),
        Command::SetDraft { pane_id, active } => Request::PaneDraft(PaneDraftParams {
            pane_id: pane_id.clone(),
            active: *active,
            idempotency_key: key(),
        }),
        Command::RemoveQueued { pane_id, prompt_id } => {
            Request::PaneQueueRemove(PaneQueueRemoveParams {
                pane_id: pane_id.clone(),
                prompt_id: prompt_id.clone(),
                idempotency_key: key(),
            })
        }
        Command::ResumeQueue { pane_id } => Request::PaneQueueResume(PaneQueueResumeParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        Command::AnswerElicitation {
            pane_id,
            request_id,
            fields,
            cancel,
        } => Request::PaneElicitationAnswer(PaneElicitationAnswerParams {
            pane_id: pane_id.clone(),
            request_id: request_id.clone(),
            fields: fields.clone(),
            cancel: cancel.then_some(true),
            idempotency_key: key(),
        }),
        Command::StartSignIn { pane_id, method_id } => {
            Request::PaneAuthStart(PaneAuthStartParams {
                pane_id: pane_id.clone(),
                auth_method_id: method_id.clone(),
                idempotency_key: key(),
            })
        }
        Command::CancelSignIn { pane_id } => Request::PaneAuthCancel(PaneAuthCancelParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        _ => return None,
    })
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

fn transcript_page(pane_id: &str, before: Option<&String>) -> Request {
    Request::PaneTranscript(PaneTranscriptParams {
        pane_id: pane_id.to_owned(),
        before: before.cloned(),
        limit: Some(TRANSCRIPT_PAGE),
    })
}

fn create(workspace_id: &str, mode: PaneMode, harness: Option<&String>) -> Request {
    Request::PaneCreate(PaneCreateParams {
        workspace_id: workspace_id.to_owned(),
        mode,
        cwd: None,
        name: None,
        cols: None,
        rows: None,
        idempotency_key: key(),
        harness: harness.cloned(),
        model: None,
        effort: None,
        permission_mode: None,
        url: None,
        incognito: None,
        path: None,
        writing_kind: None,
        writing_section: None,
    })
}

impl Command {
    /// The request that opens a pane names it, so the answer can be matched to it.
    pub fn ticket(&self) -> Option<u64> {
        match self {
            Self::CreateTerminal { ticket, .. }
            | Self::CreateAgent { ticket, .. }
            | Self::CreateTask { ticket, .. }
            | Self::DispatchTask { ticket, .. } => Some(*ticket),
            _ => None,
        }
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
                | Self::LoadHarnesses
        )
    }
}

/// What the engine's answer to `command` tells the application, for the commands that fetch
/// something.
pub fn outcome(command: &Command, result: Value) -> Result<Option<Event>> {
    Ok(match command {
        Command::LoadTranscript { pane_id } | Command::LoadOlder { pane_id, .. } => {
            let page = serde_json::from_value::<PaneTranscriptResult>(result)?;
            Some(Event::Transcript {
                pane_id: pane_id.clone(),
                items: page.items,
                next_cursor: page.next_cursor,
            })
        }
        Command::LoadTask { .. } => Some(Event::Task(TaskUpdate::Detail(Box::new(
            serde_json::from_value::<TaskShowResult>(result)?,
        )))),
        Command::CreateTask { ticket, .. } => {
            let created = serde_json::from_value::<TaskCreateResult>(result)?;
            Some(Event::TaskPane {
                task: created.task,
                ticket: *ticket,
                pane_id: created.pane_id,
            })
        }
        Command::MoveTask { .. } => Some(Event::Task(TaskUpdate::Moved(
            serde_json::from_value::<TaskMoveResult>(result)?.task,
        ))),
        Command::DispatchTask { ticket, .. } => {
            let dispatched = serde_json::from_value::<TaskDispatchResult>(result)?;
            Some(Event::TaskPane {
                task: dispatched.task,
                ticket: *ticket,
                pane_id: Some(dispatched.pane_id),
            })
        }
        Command::SetLoop { pane_id, .. } => Some(Event::LoopSet {
            pane_id: pane_id.clone(),
            config: serde_json::from_value::<LoopSetResult>(result)?.config,
        }),
        Command::LoadUsage => Some(Event::Usage(
            serde_json::from_value::<UsageListResult>(result)?.providers,
        )),
        Command::LoadHarnesses => Some(Event::Harnesses(harnesses(result)?)),
        Command::CreateTerminal { ticket, .. } | Command::CreateAgent { ticket, .. } => {
            Some(Event::Opened {
                ticket: *ticket,
                pane_id: serde_json::from_value::<PaneCreateResult>(result)?.pane_id,
            })
        }
        _ => None,
    })
}

fn harnesses(result: Value) -> Result<Vec<HarnessInfo>> {
    serde_json::from_value::<HarnessListResult>(result)
        .map(|list| list.items)
        .map_err(Error::from)
}

/// Merges queued commands that a single request can carry: keystrokes for the same pane go out
/// in one round trip, and of several resizes of one pane only the last matters. A command of
/// another kind stays in order: it is returned in `carry`.
pub fn coalesce(
    first: Command,
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    match first {
        Command::Input { pane_id, bytes } => merge_input(pane_id, bytes, queue, carry),
        Command::Resize {
            pane_id,
            rows,
            cols,
        } => merge_resize(pane_id, (rows, cols), queue, carry),
        other => other,
    }
}

fn merge_input(
    pane_id: String,
    mut bytes: Vec<u8>,
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    while bytes.len() < INPUT_CHUNK_BYTES {
        match queue.try_recv() {
            Ok(Command::Input {
                pane_id: next_pane,
                bytes: more,
            }) if next_pane == pane_id => {
                bytes.extend_from_slice(&more);
            }
            Ok(other) => {
                *carry = Some(other);
                break;
            }
            Err(_) => break,
        }
    }
    Command::Input { pane_id, bytes }
}

fn merge_resize(
    pane_id: String,
    mut size: (u16, u16),
    queue: &mpsc::Receiver<Command>,
    carry: &mut Option<Command>,
) -> Command {
    loop {
        match queue.try_recv() {
            Ok(Command::Resize {
                pane_id: next_pane,
                rows,
                cols,
            }) if next_pane == pane_id => {
                size = (rows, cols);
            }
            Ok(other) => {
                *carry = Some(other);
                break;
            }
            Err(_) => break,
        }
    }
    Command::Resize {
        pane_id,
        rows: size.0,
        cols: size.1,
    }
}

#[cfg(test)]
mod tests;
