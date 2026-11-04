//! What the home screen shows: the connection and the workspaces with their panes.

use soyeht_client::{State, state::Connection};
use soyeht_protocol::{Pane, PaneMode};

#[derive(uniffi::Enum)]
pub enum Link {
    Connecting,
    Connected,
    /// The connection ended; the core is connecting again.
    Disconnected {
        reason: String,
    },
    /// The computer no longer accepts this device: the person has to pair again.
    AccessRefused {
        reason: String,
    },
}

/// The kinds of pane the phone shows. The others are shown on the computer only.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    Terminal,
    Agent,
    Web,
    Editor,
    Writing,
}

#[derive(uniffi::Record)]
pub struct PaneRow {
    pub pane_id: String,
    pub name: String,
    pub kind: PaneKind,
    /// What the pane is doing, in the engine's words.
    pub status: String,
    pub focused: bool,
}

#[derive(uniffi::Record)]
pub struct WorkspaceRow {
    pub workspace_id: String,
    pub name: String,
    pub panes: Vec<PaneRow>,
}

#[derive(uniffi::Record)]
pub struct Overview {
    pub link: Link,
    pub workspaces: Vec<WorkspaceRow>,
    /// The last failure of something the person asked for.
    pub problem: Option<String>,
}

pub(crate) fn kind_of(pane: &Pane) -> PaneKind {
    match pane.mode {
        PaneMode::Terminal => PaneKind::Terminal,
        PaneMode::Agent => PaneKind::Agent,
        PaneMode::Web => PaneKind::Web,
        PaneMode::Editor => PaneKind::Editor,
        PaneMode::Writing => PaneKind::Writing,
    }
}

pub(crate) fn overview(state: &State, refused: Option<&str>) -> Overview {
    let focused = state.focused_pane().map(|pane| pane.pane_id.as_str());
    let workspaces = state
        .workspaces()
        .iter()
        .map(|workspace| WorkspaceRow {
            workspace_id: workspace.workspace_id.clone(),
            name: workspace.name.clone(),
            panes: state
                .panes_of(&workspace.workspace_id)
                .map(|pane| PaneRow {
                    pane_id: pane.pane_id.clone(),
                    name: pane.name.clone(),
                    kind: kind_of(pane),
                    status: pane.phrase.clone(),
                    focused: focused == Some(pane.pane_id.as_str()),
                })
                .collect(),
        })
        .collect();
    let link = match (refused, &state.connection) {
        (Some(reason), _) => Link::AccessRefused {
            reason: reason.to_owned(),
        },
        (None, Connection::Connected) => Link::Connected,
        (None, Connection::Disconnected(reason)) => Link::Disconnected {
            reason: reason.clone(),
        },
        (None, Connection::Connecting | Connection::Starting) => Link::Connecting,
    };
    Overview {
        link,
        workspaces,
        problem: state.problem().map(str::to_owned),
    }
}
