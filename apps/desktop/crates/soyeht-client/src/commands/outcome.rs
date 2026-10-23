//! What the engine's answer to a command that fetches something tells the application.

use super::Command;
use crate::{Error, Result, event::Event, tasks::TaskUpdate};
use serde_json::Value;
use soyeht_protocol::{
    EngineReadinessResult, HarnessInfo, HarnessListResult, LoopSetResult, PaneCommandsListResult,
    PaneCreateResult, PaneTranscriptResult, SkillsListResult, TaskCreateResult, TaskDispatchResult,
    TaskMoveResult, TaskShowResult, UsageListResult, WorkspaceCreateResult,
};

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
        Command::LoadHarnesses { .. } => Some(Event::Harnesses(harnesses(result)?)),
        Command::LoadReadiness => Some(Event::Readiness(serde_json::from_value::<
            EngineReadinessResult,
        >(result)?)),
        Command::LoadSkills { workspace_id } => Some(Event::Skills {
            workspace_id: workspace_id.clone(),
            listing: serde_json::from_value::<SkillsListResult>(result)?,
        }),
        Command::LoadCommands { pane_id } => Some(Event::PaneCommands {
            pane_id: pane_id.clone(),
            items: serde_json::from_value::<PaneCommandsListResult>(result)?.items,
        }),
        Command::CreateWorkspace { ticket, .. } => Some(Event::WorkspaceOpened {
            ticket: *ticket,
            workspace: serde_json::from_value::<WorkspaceCreateResult>(result)?.workspace,
        }),
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
