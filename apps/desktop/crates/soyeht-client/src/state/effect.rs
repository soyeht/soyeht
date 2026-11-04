//! What the replica asks its application to do.

use crate::commands::Command;
use soyeht_protocol::WorkspacePaneLayoutEnvelope;

/// Work the application must do because the state changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Ask the engine for this terminal's output.
    SubscribeTerminal(String),
    /// The replica can no longer be trusted: reconnect and start again from a fresh snapshot.
    Resync,
    /// Fetch an agent pane's transcript.
    LoadTranscript(String),
    /// Fetch the usage of every provider.
    LoadUsage,
    /// Fetch the agent harnesses the engine can start.
    LoadHarnesses,
    /// Fetch the skills again: they changed on the engine, or a pane that lists them arrived.
    /// `None` is the user's alone.
    LoadSkills(Option<String>),
    /// Fetch the commands an agent pane's harness advertised.
    LoadCommands(String),
    /// Ask what this machine has of what the app needs.
    LoadReadiness,
    /// Ask whether the engine has its link to the supervisor of the terminals: a window that
    /// connects while the supervisor is down was not there to hear `ptyd.lost`.
    LoadSupervisor,
    /// Fetch one task again, with its detail.
    LoadTask {
        workspace_id: String,
        task_id: String,
    },
    /// The pane the person asked to open has arrived: it takes the keyboard.
    FocusPane(String),
    /// Store this arrangement for a workspace: a pane was split or a divider moved.
    StoreLayout {
        workspace_id: String,
        layout: WorkspacePaneLayoutEnvelope,
    },
    /// Send this command to the engine.
    Send(Command),
}

impl Effect {
    /// The requests this effect is, when it is nothing but a request to the engine: what every
    /// application does for it. The rest (`Resync`, `FocusPane`, `StoreLayout`, `Send`) differ
    /// from one application to the next.
    pub fn commands(&self) -> Vec<Command> {
        match self {
            Self::SubscribeTerminal(pane_id) => vec![Command::Subscribe {
                pane_id: pane_id.clone(),
            }],
            // A pane's queue of waiting messages is loaded with its transcript.
            Self::LoadTranscript(pane_id) => vec![
                Command::LoadQueue {
                    pane_id: pane_id.clone(),
                },
                Command::LoadTranscript {
                    pane_id: pane_id.clone(),
                },
            ],
            Self::LoadUsage => vec![Command::LoadUsage],
            Self::LoadHarnesses => vec![Command::LoadHarnesses { rescan: false }],
            Self::LoadSkills(workspace_id) => vec![Command::LoadSkills {
                workspace_id: workspace_id.clone(),
            }],
            Self::LoadCommands(pane_id) => vec![Command::LoadCommands {
                pane_id: pane_id.clone(),
            }],
            Self::LoadReadiness => vec![Command::LoadReadiness],
            Self::LoadSupervisor => vec![Command::LoadSupervisor],
            Self::LoadTask {
                workspace_id,
                task_id,
            } => vec![Command::LoadTask {
                workspace_id: workspace_id.clone(),
                task_id: task_id.clone(),
            }],
            Self::Resync | Self::FocusPane(_) | Self::StoreLayout { .. } | Self::Send(_) => {
                Vec::new()
            }
        }
    }
}
