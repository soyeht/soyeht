//! What the home screen shows: the connection and the workspaces with their panes.

use soyeht_client::{State, state::Connection};
use soyeht_protocol::{Pane, PaneMode, PaneState, client_policy::StateDot};

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

/// The palette role of a pane state's dot (`x-clientPolicy.paneStates`), which the view resolves to a color.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum PaneDot {
    Success,
    Accent,
    Warning,
    Muted,
    Secondary,
}

#[derive(uniffi::Record)]
pub struct PaneRow {
    pub pane_id: String,
    pub name: String,
    pub kind: PaneKind,
    /// What the pane is doing, in the engine's words.
    pub status: String,
    pub focused: bool,
    pub dot: PaneDot,
    /// The state in a word for the header ("working", "waiting for you"), as the other clients write it.
    pub state_title: String,
    /// The agent is waiting for the person: the header's word takes the accent.
    pub needs_you: bool,
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

/// The state in the words of the header: a pane that needs a sign-in says so, a started session is "ready",
/// else the state's wire word with each underscore replaced by a blank, as the other clients write it.
fn state_title(pane: &Pane) -> String {
    let agent = pane.agent.as_ref();
    if agent.is_some_and(|agent| !agent.auth_methods.is_empty()) {
        return "Sign in".to_owned();
    }
    if pane.state == PaneState::Starting && agent.is_some_and(|agent| agent.session_id.is_some()) {
        return "ready".to_owned();
    }
    pane.state.as_str().replace('_', " ")
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
                    dot: match pane.state.dot() {
                        StateDot::Success => PaneDot::Success,
                        StateDot::Accent => PaneDot::Accent,
                        StateDot::Warning => PaneDot::Warning,
                        StateDot::Muted => PaneDot::Muted,
                        StateDot::Secondary => PaneDot::Secondary,
                    },
                    state_title: state_title(pane),
                    needs_you: pane.state == PaneState::WaitingForYou
                        || state
                            .agent(&pane.pane_id)
                            .is_some_and(|view| view.permission.is_some()),
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
