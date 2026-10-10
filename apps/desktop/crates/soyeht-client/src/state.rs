//! The client's replica of the engine: workspaces, panes, selection and one screen per
//! terminal pane. It is a plain value that applies [`Event`]s, so every rule here is testable
//! without a window or a connection.

use crate::{
    agent::AgentView, event::Event, screen::TerminalScreen, snapshot::Snapshot,
    streams::StreamOutput,
};
use serde_json::Value;
use soyeht_protocol::{
    EventName, HarnessInfo, Pane, PaneClosedEvent, PaneCreatedEvent, PaneMode,
    PaneStateChangedEvent, PaneUpdatedEvent, UsageReading, Workspace, WorkspaceClosedEvent,
    WorkspaceCreatedEvent, WorkspaceLayoutUpdatedEvent, WorkspacePaneLayoutAxis,
    WorkspacePaneLayoutEnvelope, WorkspaceRenamedEvent,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    Disconnected(String),
}

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
    /// The pane the person asked to open has arrived: it takes the keyboard.
    FocusPane(String),
    /// Store this arrangement for a workspace: a pane was split.
    StoreLayout {
        workspace_id: String,
        layout: WorkspacePaneLayoutEnvelope,
    },
}

/// A pane the person asked to open, until it arrives: named by the ticket of the request, so a
/// pane that someone else opens is never mistaken for it. A split also says which pane the new
/// one goes beside.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Expected {
    pub(crate) ticket: u64,
    pub(crate) workspace_id: String,
    pub(crate) split: Option<(String, WorkspacePaneLayoutAxis)>,
    /// Known once the engine has answered the request.
    pub(crate) pane_id: Option<String>,
}

pub struct State {
    pub connection: Connection,
    pub problem: Option<String>,
    pub(crate) workspaces: Vec<Workspace>,
    pub(crate) panes: Vec<Pane>,
    selected_workspace: Option<String>,
    focused_pane: Option<String>,
    screens: HashMap<String, TerminalScreen>,
    gaps: HashSet<String>,
    pub(crate) agents: HashMap<String, AgentView>,
    pub(crate) usage: Vec<UsageReading>,
    pub(crate) harnesses: Vec<HarnessInfo>,
    pub(crate) expected: Vec<Expected>,
    /// Panes closed since the last snapshot. The engine may still announce one more update
    /// about a pane as it closes; it must not bring the pane back.
    closed: HashSet<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            connection: Connection::Connecting,
            problem: None,
            workspaces: Vec::new(),
            panes: Vec::new(),
            selected_workspace: None,
            focused_pane: None,
            screens: HashMap::new(),
            gaps: HashSet::new(),
            agents: HashMap::new(),
            usage: Vec::new(),
            harnesses: Vec::new(),
            expected: Vec::new(),
            closed: HashSet::new(),
        }
    }
}

impl State {
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub fn panes_of<'a>(&'a self, workspace_id: &'a str) -> impl Iterator<Item = &'a Pane> {
        self.panes
            .iter()
            .filter(move |pane| pane.workspace_id == workspace_id)
    }

    pub fn selected_workspace(&self) -> Option<&Workspace> {
        let id = self.selected_workspace.as_deref()?;
        self.workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == id)
    }

    pub fn focused_pane(&self) -> Option<&Pane> {
        let id = self.focused_pane.as_deref()?;
        self.panes.iter().find(|pane| pane.pane_id == id)
    }

    pub fn screen(&self, pane_id: &str) -> Option<&TerminalScreen> {
        self.screens.get(pane_id)
    }

    pub fn screen_mut(&mut self, pane_id: &str) -> Option<&mut TerminalScreen> {
        self.screens.get_mut(pane_id)
    }

    /// True after the engine skipped output it no longer retains.
    pub fn has_gap(&self, pane_id: &str) -> bool {
        self.gaps.contains(pane_id)
    }

    pub fn select_workspace(&mut self, workspace_id: &str) {
        if self
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == workspace_id)
        {
            self.selected_workspace = Some(workspace_id.to_owned());
            self.focused_pane = self.default_focus(workspace_id);
        }
    }

    pub fn focus_pane(&mut self, pane_id: &str) {
        if let Some(pane) = self.panes.iter().find(|pane| pane.pane_id == pane_id) {
            self.selected_workspace = Some(pane.workspace_id.clone());
            self.focused_pane = Some(pane_id.to_owned());
        }
    }

    /// Fits a terminal's screen to the room its view has. Returns the new size when it changed,
    /// so the caller can tell the engine.
    pub fn fit_terminal(&mut self, pane_id: &str, rows: u16, cols: u16) -> Option<(u16, u16)> {
        let screen = self.screens.get_mut(pane_id)?;
        let wanted = (rows.max(1), cols.max(1));
        if screen.size() == wanted {
            return None;
        }
        screen.resize(wanted.0, wanted.1);
        Some(wanted)
    }

    pub fn apply(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Connected(snapshot) => {
                self.connection = Connection::Connected;
                self.problem = None;
                self.adopt(snapshot)
            }
            Event::Disconnected(reason) => {
                self.connection = Connection::Disconnected(reason);
                // A request in flight is lost with the connection.
                self.expected.clear();
                Vec::new()
            }
            Event::Notification { name, params } => self.notify(name, params),
            Event::Terminal(output) => {
                self.terminal_output(output);
                Vec::new()
            }
            Event::Problem(message) => {
                self.problem = Some(message);
                Vec::new()
            }
            Event::Transcript { pane_id, items } => {
                if self.agents.contains_key(&pane_id)
                    && let Err(error) = self.agents.entry(pane_id).or_default().merge_page(items)
                {
                    self.problem = Some(error.to_string());
                }
                Vec::new()
            }
            Event::Opened { ticket, pane_id } => self.opened(ticket, pane_id),
            Event::OpenFailed { ticket } => {
                self.expected.retain(|expected| expected.ticket != ticket);
                Vec::new()
            }
            Event::Usage(readings) => {
                self.usage = readings;
                Vec::new()
            }
            Event::Harnesses(items) => {
                self.harnesses = items;
                Vec::new()
            }
        }
    }

    fn adopt(&mut self, snapshot: Snapshot) -> Vec<Effect> {
        for workspace in &snapshot.workspaces {
            self.check_layout(workspace);
        }
        self.workspaces = snapshot.workspaces;
        self.panes = snapshot.panes;
        self.expected.clear();
        self.screens.retain(|id, _| {
            self.panes
                .iter()
                .any(|pane| is_terminal(pane) && pane.pane_id == *id)
        });
        self.gaps.clear();
        self.closed.clear();
        let mut effects = Vec::new();
        for pane in self.panes.iter().filter(|pane| is_terminal(pane)) {
            self.screens
                .entry(pane.pane_id.clone())
                .or_insert_with(|| screen_for(pane));
            effects.push(Effect::SubscribeTerminal(pane.pane_id.clone()));
        }
        effects.extend(self.adopt_agents(&snapshot.requests));
        let focus = snapshot.focus;
        let keep =
            |id: &Option<String>, exists: &dyn Fn(&str) -> bool| id.clone().filter(|id| exists(id));
        let workspace_exists = |id: &str| self.workspaces.iter().any(|w| w.workspace_id == id);
        let pane_exists = |id: &str| self.panes.iter().any(|p| p.pane_id == id);
        self.selected_workspace = keep(&self.selected_workspace, &workspace_exists)
            .or_else(|| {
                keep(
                    &focus.as_ref().and_then(|f| f.workspace_id.clone()),
                    &workspace_exists,
                )
            })
            .or_else(|| self.workspaces.first().map(|w| w.workspace_id.clone()));
        self.focused_pane = keep(&self.focused_pane, &pane_exists).or_else(|| {
            keep(
                &focus.as_ref().and_then(|f| f.pane_id.clone()),
                &pane_exists,
            )
        });
        self.repair_selection();
        effects
    }

    fn default_focus(&self, workspace_id: &str) -> Option<String> {
        self.panes_of(workspace_id)
            .find(|pane| is_terminal(pane))
            .or_else(|| self.panes_of(workspace_id).next())
            .map(|pane| pane.pane_id.clone())
    }

    /// Keeps the selection pointing at things that exist and at each other.
    fn repair_selection(&mut self) {
        if let Some(pane) = self.focused_pane()
            && self.selected_workspace.as_deref() != Some(pane.workspace_id.as_str())
        {
            self.selected_workspace = Some(pane.workspace_id.clone());
        }
        if self.focused_pane.is_none()
            && let Some(workspace_id) = self.selected_workspace.clone()
        {
            self.focused_pane = self.default_focus(&workspace_id);
        }
    }

    /// An event the client cannot read has already moved the event cursor, so ignoring it
    /// would leave the replica wrong for good. Say so and ask for a fresh snapshot, as the
    /// Swift client does by dropping the connection.
    fn notify(&mut self, name: EventName, params: Value) -> Vec<Effect> {
        match self.apply_notification(name, params) {
            Ok(effects) => effects,
            Err(error) => {
                self.problem = Some(format!(
                    "The engine sent an unreadable {} event ({error}). Reloading its state.",
                    name.as_str()
                ));
                vec![Effect::Resync]
            }
        }
    }

    fn apply_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            EventName::WorkspaceCreated => self.upsert_workspace(
                serde_json::from_value::<WorkspaceCreatedEvent>(params)?.workspace,
            ),
            EventName::WorkspaceRenamed => self.upsert_workspace(
                serde_json::from_value::<WorkspaceRenamedEvent>(params)?.workspace,
            ),
            EventName::WorkspaceLayoutUpdated => self.upsert_workspace(
                serde_json::from_value::<WorkspaceLayoutUpdatedEvent>(params)?.workspace,
            ),
            EventName::WorkspaceClosed => {
                self.close_workspace(
                    &serde_json::from_value::<WorkspaceClosedEvent>(params)?.workspace_id,
                );
                Vec::new()
            }
            EventName::PaneCreated => {
                self.upsert_pane(serde_json::from_value::<PaneCreatedEvent>(params)?.pane)
            }
            EventName::PaneRenamed | EventName::PaneUpdated => {
                self.upsert_pane(serde_json::from_value::<PaneUpdatedEvent>(params)?.pane)
            }
            EventName::PaneClosed => {
                self.remove_pane(&serde_json::from_value::<PaneClosedEvent>(params)?.pane_id);
                Vec::new()
            }
            EventName::PaneStateChanged => {
                self.change_pane_state(serde_json::from_value(params)?);
                Vec::new()
            }
            _ => self.agent_notification(name, params)?,
        })
    }

    fn close_workspace(&mut self, workspace_id: &str) {
        self.workspaces.retain(|w| w.workspace_id != workspace_id);
        if self.selected_workspace.as_deref() == Some(workspace_id) {
            self.selected_workspace = self.workspaces.first().map(|w| w.workspace_id.clone());
            self.focused_pane = None;
            self.repair_selection();
        }
    }

    fn change_pane_state(&mut self, event: PaneStateChangedEvent) {
        if let Some(pane) = self.panes.iter_mut().find(|p| p.pane_id == event.pane_id) {
            pane.state = event.state;
            pane.source = event.source;
            pane.confidence = event.confidence;
            pane.phrase = event.phrase;
        }
    }

    fn upsert_workspace(&mut self, workspace: Workspace) -> Vec<Effect> {
        self.check_layout(&workspace);
        match self
            .workspaces
            .iter_mut()
            .find(|known| known.workspace_id == workspace.workspace_id)
        {
            Some(known) => *known = workspace,
            None => self.workspaces.push(workspace),
        }
        if self.selected_workspace.is_none() {
            self.selected_workspace = self.workspaces.first().map(|w| w.workspace_id.clone());
            self.repair_selection();
        }
        Vec::new()
    }

    fn upsert_pane(&mut self, pane: Pane) -> Vec<Effect> {
        if self.closed.contains(&pane.pane_id) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        let pane_id = pane.pane_id.clone();
        let arrived = !self.panes.iter().any(|known| known.pane_id == pane_id);
        if pane.mode == PaneMode::Agent && !self.agents.contains_key(&pane.pane_id) {
            self.agents
                .insert(pane.pane_id.clone(), AgentView::default());
            effects.push(Effect::LoadTranscript(pane.pane_id.clone()));
        }
        let new_terminal = is_terminal(&pane) && !self.screens.contains_key(&pane.pane_id);
        if new_terminal {
            self.screens.insert(pane.pane_id.clone(), screen_for(&pane));
            effects.push(Effect::SubscribeTerminal(pane.pane_id.clone()));
        }
        match self
            .panes
            .iter_mut()
            .find(|known| known.pane_id == pane.pane_id)
        {
            Some(known) => *known = pane,
            None => self.panes.push(pane),
        }
        self.repair_selection();
        if arrived {
            effects.extend(self.settle(&pane_id));
        }
        effects
    }

    fn remove_pane(&mut self, pane_id: &str) {
        self.closed.insert(pane_id.to_owned());
        self.panes.retain(|pane| pane.pane_id != pane_id);
        self.screens.remove(pane_id);
        self.agents.remove(pane_id);
        self.gaps.remove(pane_id);
        if self.focused_pane.as_deref() == Some(pane_id) {
            self.focused_pane = None;
            self.repair_selection();
        }
    }

    fn terminal_output(&mut self, output: StreamOutput) {
        match output {
            StreamOutput::Reset { pane_id } => {
                self.gaps.remove(&pane_id);
                if let Some(screen) = self.screens.get_mut(&pane_id) {
                    screen.reset();
                }
            }
            StreamOutput::Bytes { pane_id, bytes } => {
                if let Some(screen) = self.screens.get_mut(&pane_id) {
                    screen.feed(&bytes);
                }
            }
            StreamOutput::Gap { pane_id } => {
                self.gaps.insert(pane_id);
            }
        }
    }
}

fn is_terminal(pane: &Pane) -> bool {
    pane.mode == PaneMode::Terminal
}

fn screen_for(pane: &Pane) -> TerminalScreen {
    let rows = u16::try_from(pane.rows).unwrap_or(24);
    let cols = u16::try_from(pane.cols).unwrap_or(80);
    TerminalScreen::new(rows, cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{pane, pane_json, workspace, workspace_json};
    use serde_json::json;
    use soyeht_protocol::ClientFocus;

    fn snapshot(workspaces: &[&str], panes: &[(&str, &str, &str)]) -> Snapshot {
        Snapshot {
            workspaces: workspaces.iter().map(|id| workspace(id)).collect(),
            panes: panes
                .iter()
                .map(|(id, ws, mode)| pane(id, ws, mode))
                .collect(),
            focus: None,
            cursor: 1,
            requests: Vec::new(),
        }
    }

    /// A terminal in each of two workspaces.
    fn across_workspaces() -> Snapshot {
        snapshot(
            &["w1", "w2"],
            &[("t1", "w1", "terminal"), ("t2", "w2", "terminal")],
        )
    }

    /// Two terminals in one workspace.
    fn side_by_side() -> Snapshot {
        snapshot(&["w"], &[("t1", "w", "terminal"), ("t2", "w", "terminal")])
    }

    fn connected(snapshot: Snapshot) -> (State, Vec<Effect>) {
        let mut state = State::default();
        let effects = state.apply(Event::Connected(snapshot));
        (state, effects)
    }

    fn notification(name: EventName, params: Value) -> Event {
        Event::Notification { name, params }
    }

    #[test]
    fn a_snapshot_lists_workspaces_and_panes_and_subscribes_every_terminal() {
        let (state, effects) = connected(snapshot(
            &["w1", "w2"],
            &[
                ("t1", "w1", "terminal"),
                ("a1", "w1", "agent"),
                ("t2", "w2", "terminal"),
            ],
        ));
        assert_eq!(state.connection, Connection::Connected);
        assert_eq!(state.workspaces().len(), 2);
        assert_eq!(state.panes_of("w1").count(), 2);
        assert_eq!(
            effects,
            [
                Effect::SubscribeTerminal("t1".into()),
                Effect::SubscribeTerminal("t2".into()),
                Effect::LoadUsage,
                Effect::LoadHarnesses,
                Effect::LoadTranscript("a1".into()),
            ]
        );
        assert!(state.screen("t1").is_some() && state.screen("a1").is_none());
        assert_eq!(state.screen("t1").unwrap().size(), (24, 80));
    }

    #[test]
    fn selection_defaults_to_the_engine_focus_then_the_first_workspace() {
        let mut shot = across_workspaces();
        shot.focus = Some(ClientFocus {
            workspace_id: Some("w2".into()),
            pane_id: Some("t2".into()),
        });
        let (state, _) = connected(shot);
        assert_eq!(state.selected_workspace().unwrap().workspace_id, "w2");
        assert_eq!(state.focused_pane().unwrap().pane_id, "t2");

        let (state, _) = connected(snapshot(&["w1", "w2"], &[("t1", "w1", "terminal")]));
        assert_eq!(state.selected_workspace().unwrap().workspace_id, "w1");
        assert_eq!(state.focused_pane().unwrap().pane_id, "t1");
    }

    #[test]
    fn reconnecting_keeps_the_selection_and_resubscribes() {
        let (mut state, _) = connected(across_workspaces());
        state.focus_pane("t2");
        state.apply(Event::Disconnected("lost".into()));
        assert_eq!(state.connection, Connection::Disconnected("lost".into()));
        let effects = state.apply(Event::Connected(across_workspaces()));
        assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
        assert!(effects.contains(&Effect::SubscribeTerminal("t1".into())));
        assert!(effects.contains(&Effect::SubscribeTerminal("t2".into())));
    }

    #[test]
    fn terminal_output_reaches_only_its_pane_and_reset_clears_it() {
        let (mut state, _) = connected(side_by_side());
        state.apply(Event::Terminal(StreamOutput::Bytes {
            pane_id: "t1".into(),
            bytes: b"hi".to_vec(),
        }));
        assert!(state.screen("t1").unwrap().contents().starts_with("hi"));
        assert!(state.screen("t2").unwrap().contents().trim().is_empty());
        state.apply(Event::Terminal(StreamOutput::Reset {
            pane_id: "t1".into(),
        }));
        assert!(state.screen("t1").unwrap().contents().trim().is_empty());
    }

    #[test]
    fn gaps_are_remembered_until_the_next_reset() {
        let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
        state.apply(Event::Terminal(StreamOutput::Gap {
            pane_id: "t1".into(),
        }));
        assert!(state.has_gap("t1"));
        state.apply(Event::Terminal(StreamOutput::Reset {
            pane_id: "t1".into(),
        }));
        assert!(!state.has_gap("t1"));
    }

    #[test]
    fn created_panes_appear_and_new_terminals_are_subscribed() {
        let (mut state, _) = connected(snapshot(&["w"], &[]));
        let effects = state.apply(notification(
            EventName::PaneCreated,
            json!({"eventId": "2", "at": "now", "pane": pane_json("t9", "w", "terminal")}),
        ));
        assert_eq!(effects, [Effect::SubscribeTerminal("t9".into())]);
        assert_eq!(state.focused_pane().unwrap().pane_id, "t9");
        assert!(state.screen("t9").is_some());
    }

    #[test]
    fn renamed_panes_update_in_place_without_a_second_subscription() {
        let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
        let mut renamed = pane_json("t1", "w", "terminal");
        renamed["name"] = json!("build");
        let effects = state.apply(notification(
            EventName::PaneRenamed,
            json!({"eventId": "3", "at": "now", "pane": renamed}),
        ));
        assert!(effects.is_empty());
        assert_eq!(state.panes()[0].name, "build");
        assert_eq!(state.panes().len(), 1);
    }

    #[test]
    fn an_update_that_trails_a_close_does_not_bring_the_pane_back() {
        let (mut state, _) = connected(side_by_side());
        state.apply(notification(
            EventName::PaneClosed,
            json!({"eventId": "4", "at": "now", "paneId": "t1", "workspaceId": "w"}),
        ));
        let mut dead = pane_json("t1", "w", "terminal");
        dead["state"] = json!("dead");
        state.apply(notification(
            EventName::PaneUpdated,
            json!({"eventId": "5", "at": "now", "pane": dead}),
        ));
        assert_eq!(state.panes().len(), 1);
        assert!(state.screen("t1").is_none());
    }

    #[test]
    fn closing_the_focused_pane_moves_focus_to_a_neighbour() {
        let (mut state, _) = connected(side_by_side());
        assert_eq!(state.focused_pane().unwrap().pane_id, "t1");
        state.apply(notification(
            EventName::PaneClosed,
            json!({"eventId": "4", "at": "now", "paneId": "t1", "workspaceId": "w"}),
        ));
        assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
        assert!(state.screen("t1").is_none());
    }

    #[test]
    fn pane_state_changes_update_the_pane() {
        let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
        state.apply(notification(
            EventName::PaneStateChanged,
            json!({"eventId": "5", "at": "now", "paneId": "t1", "state": "working",
                   "confidence": 0.9, "phrase": "Building", "source": "osc"}),
        ));
        assert_eq!(state.panes()[0].phrase, "Building");
    }

    #[test]
    fn workspace_events_add_rename_and_close() {
        let (mut state, _) = connected(snapshot(&[], &[]));
        assert!(state.selected_workspace().is_none());
        state.apply(notification(
            EventName::WorkspaceCreated,
            json!({"eventId": "6", "at": "now", "workspace": workspace_json("w1")}),
        ));
        assert_eq!(state.selected_workspace().unwrap().workspace_id, "w1");
        let mut renamed = workspace_json("w1");
        renamed["name"] = json!("api");
        state.apply(notification(
            EventName::WorkspaceRenamed,
            json!({"eventId": "7", "at": "now", "workspace": renamed}),
        ));
        assert_eq!(state.workspaces()[0].name, "api");
        state.apply(notification(
            EventName::WorkspaceClosed,
            json!({"eventId": "8", "at": "now", "workspaceId": "w1"}),
        ));
        assert!(state.workspaces().is_empty());
        assert!(state.selected_workspace().is_none());
    }

    #[test]
    fn unrelated_notifications_change_nothing() {
        let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
        let effects = state.apply(notification(EventName::TaskCreated, json!({})));
        assert!(effects.is_empty() && state.problem.is_none());
        assert_eq!(state.panes().len(), 1);
    }

    #[test]
    fn an_unreadable_state_event_reports_a_problem_and_asks_for_a_fresh_snapshot() {
        for name in [
            EventName::WorkspaceCreated,
            EventName::WorkspaceClosed,
            EventName::PaneCreated,
            EventName::PaneUpdated,
            EventName::PaneClosed,
            EventName::PaneStateChanged,
        ] {
            let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
            let effects = state.apply(notification(name, json!({"nonsense": true})));
            assert_eq!(effects, [Effect::Resync], "{}", name.as_str());
            assert!(
                state
                    .problem
                    .as_deref()
                    .is_some_and(|p| p.contains(name.as_str())),
                "{}",
                name.as_str()
            );
            assert_eq!(state.panes().len(), 1);
        }
    }

    #[test]
    fn fitting_a_terminal_reports_only_real_size_changes() {
        let (mut state, _) = connected(snapshot(&["w"], &[("t1", "w", "terminal")]));
        assert_eq!(state.fit_terminal("t1", 24, 80), None);
        assert_eq!(state.fit_terminal("t1", 30, 100), Some((30, 100)));
        assert_eq!(state.screen("t1").unwrap().size(), (30, 100));
        assert_eq!(state.fit_terminal("missing", 30, 100), None);
    }

    #[test]
    fn selecting_a_workspace_focuses_its_first_terminal() {
        let (mut state, _) = connected(across_workspaces());
        state.select_workspace("w2");
        assert_eq!(state.focused_pane().unwrap().pane_id, "t2");
        state.select_workspace("missing");
        assert_eq!(state.selected_workspace().unwrap().workspace_id, "w2");
    }

    #[test]
    fn problems_are_kept_until_the_next_connection() {
        let mut state = State::default();
        state.apply(Event::Problem("boom".into()));
        assert_eq!(state.problem.as_deref(), Some("boom"));
        state.apply(Event::Connected(Snapshot::default()));
        assert_eq!(state.problem, None);
    }
}
