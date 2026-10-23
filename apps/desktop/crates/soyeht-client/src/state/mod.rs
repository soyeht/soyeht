//! The client's replica of the engine: workspaces, panes, selection and one screen per
//! terminal pane. It is a plain value that applies [`Event`]s, so every rule here is testable
//! without a window or a connection.

use crate::{
    Command, agent::AgentView, catalogue::Catalogue, editor::Editors, event::Event,
    new_agent::NewAgent, preferences::Preferences, screen::TerminalScreen, snapshot::Snapshot,
    streams::StreamOutput, writing::Writings,
};
use serde_json::Value;
use soyeht_protocol::{
    AgentCommand, AttentionItem, EngineReadinessResult, EventName, HarnessInfo, LoopConfig, Pane,
    PaneClosedEvent, PaneCreatedEvent, PaneMode, PaneStateChangedEvent, PaneUpdatedEvent,
    SkillsListResult, Task, TaskShowResult, UsageReading, Workspace, WorkspaceClosedEvent,
    WorkspaceCreatedEvent, WorkspaceLayoutUpdatedEvent, WorkspacePaneLayoutAxis,
    WorkspacePaneLayoutEnvelope, WorkspaceRenamedEvent,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    /// No engine answered, and the window is starting one.
    Starting,
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
    /// Fetch the skills again: they changed on the engine, or a pane that lists them arrived.
    /// `None` is the user's alone.
    LoadSkills(Option<String>),
    /// Fetch the commands an agent pane's harness advertised.
    LoadCommands(String),
    /// Ask what this machine has of what the app needs.
    LoadReadiness,
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
    /// Panes of the workspace that arrived before the answer named the pane: one of them is
    /// the new pane, and until the answer says which, none of them is drawn (a split pane
    /// must never show in the automatic place first and then move).
    pub(crate) arrivals: Vec<String>,
    /// The agent started from the first-run form, whose first message goes once its pane is here.
    pub(crate) start: Option<NewAgent>,
}

pub struct State {
    pub connection: Connection,
    pub problem: Option<String>,
    /// A standing note about this client rather than about a request (it draws on a software
    /// renderer): a connection, which clears `problem`, leaves it. The person dismisses it.
    pub notice: Option<String>,
    /// What the weights of an arrangement are measured with; the window sets it.
    pub(crate) sizing: crate::layout::Sizing,
    pub(crate) workspaces: Vec<Workspace>,
    pub(crate) panes: Vec<Pane>,
    selected_workspace: Option<String>,
    focused_pane: Option<String>,
    screens: HashMap<String, TerminalScreen>,
    gaps: HashSet<String>,
    pub(crate) agents: HashMap<String, AgentView>,
    /// The loop of each pane that has one.
    pub(crate) loops: HashMap<String, LoopConfig>,
    /// The tasks of every workspace, in the order they arrived.
    pub(crate) tasks: Vec<Task>,
    /// The detail of each task the person opened.
    pub(crate) task_details: HashMap<String, TaskShowResult>,
    /// What needs the person's attention, by workspace.
    pub(crate) attention: HashMap<String, Vec<AttentionItem>>,
    pub(crate) usage: Vec<UsageReading>,
    pub(crate) harnesses: Vec<HarnessInfo>,
    /// The engine has answered the request for the harnesses at least once.
    pub(crate) harnesses_loaded: bool,
    pub(crate) catalogues: HashMap<String, Catalogue>,
    /// Why the models of a harness could not be read, until they are.
    pub(crate) catalogue_failures: HashMap<String, String>,
    pub(crate) preferences: Preferences,
    /// The skills listed so far, by workspace; the empty key holds the user's alone.
    pub(crate) skills: HashMap<String, SkillsListResult>,
    /// The commands each agent pane's harness advertised.
    pub(crate) commands: HashMap<String, Vec<AgentCommand>>,
    pub(crate) readiness: Option<EngineReadinessResult>,
    /// The files open in editor panes, the tabs and the project trees.
    pub editors: Editors,
    /// The documents open in writing panes and the lists of the sections.
    pub writings: Writings,
    pub(crate) expected: Vec<Expected>,
    /// The arrangement a divider is being dragged to, shown in place of the stored one.
    pub(crate) layout_draft: Option<layout_draft::LayoutDraft>,
    /// The layout of each workspace as the engine last sent it: what comes back when the engine
    /// refuses the one that was drawn at once.
    pub(crate) confirmed_layouts: HashMap<String, Value>,
    /// The agents to start in workspaces the engine has not made yet, by the ticket of the
    /// request that makes the workspace.
    pub(crate) starts: HashMap<u64, NewAgent>,
    /// The agent the person asked for that could not start, until the window takes it back to
    /// its form: what they typed is not lost.
    failed_start: Option<NewAgent>,
    /// Panes closed since the last snapshot. The engine may still announce one more update
    /// about a pane as it closes; it must not bring the pane back.
    closed: HashSet<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            connection: Connection::Connecting,
            problem: None,
            notice: None,
            sizing: crate::layout::Sizing::default(),
            workspaces: Vec::new(),
            panes: Vec::new(),
            selected_workspace: None,
            focused_pane: None,
            screens: HashMap::new(),
            gaps: HashSet::new(),
            agents: HashMap::new(),
            loops: HashMap::new(),
            tasks: Vec::new(),
            task_details: HashMap::new(),
            attention: HashMap::new(),
            usage: Vec::new(),
            harnesses: Vec::new(),
            harnesses_loaded: false,
            catalogues: HashMap::new(),
            catalogue_failures: HashMap::new(),
            preferences: Preferences::default(),
            skills: HashMap::new(),
            commands: HashMap::new(),
            readiness: None,
            editors: Editors::default(),
            writings: Writings::default(),
            expected: Vec::new(),
            layout_draft: None,
            confirmed_layouts: HashMap::new(),
            starts: HashMap::new(),
            failed_start: None,
            closed: HashSet::new(),
        }
    }
}

impl State {
    /// The state measuring its arrangements with `sizing`.
    pub fn with_sizing(mut self, sizing: crate::layout::Sizing) -> Self {
        self.sizing = sizing;
        self
    }

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

    pub fn workspace(&self, workspace_id: &str) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)
    }

    pub fn selected_workspace(&self) -> Option<&Workspace> {
        self.workspace(self.selected_workspace.as_deref()?)
    }

    pub fn pane(&self, pane_id: &str) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.pane_id == pane_id)
    }

    pub fn focused_pane(&self) -> Option<&Pane> {
        self.pane(self.focused_pane.as_deref()?)
    }

    /// The id of the selected workspace.
    pub fn selected_workspace_id(&self) -> Option<String> {
        self.selected_workspace()
            .map(|workspace| workspace.workspace_id.clone())
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

    /// Whether applying `event` can change what the window draws. Terminal output cannot while its
    /// pane is in a workspace that is not the selected one: the replica of its screen keeps up
    /// with the output, and the pane draws that replica when its workspace is selected.
    pub fn changes_view(&self, event: &Event) -> bool {
        let Event::Terminal(output) = event else {
            return true;
        };
        let selected = self.selected_workspace.as_deref();
        self.panes
            .iter()
            .find(|pane| pane.pane_id == output.pane_id())
            .is_some_and(|pane| selected == Some(pane.workspace_id.as_str()))
    }

    /// Selects a workspace and focuses its first terminal. It reads and writes only this replica:
    /// every workspace's panes already hold their screens, so the new workspace draws at once
    /// and the engine is told afterwards.
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
            self.editors.activated(pane);
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
                self.forget_requests();
                self.editors.connection_lost();
                Vec::new()
            }
            Event::Starting => {
                self.connection = Connection::Starting;
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
            Event::Transcript {
                pane_id,
                items,
                next_cursor,
            } => {
                if let Some(view) = self.agents.get_mut(&pane_id) {
                    view.cards.older = next_cursor;
                    if let Err(error) = view.merge_page(items) {
                        self.problem = Some(error.to_string());
                    }
                }
                Vec::new()
            }
            Event::Task(update) => self.apply_task_update(update),
            Event::TaskPane {
                task,
                ticket,
                pane_id,
            } => {
                self.upsert_task(task);
                match pane_id {
                    Some(pane_id) => self.opened(ticket, pane_id),
                    None => {
                        self.expected.retain(|expected| expected.ticket != ticket);
                        Vec::new()
                    }
                }
            }
            Event::LoopSet { pane_id, config } => {
                self.set_loop(pane_id, config);
                Vec::new()
            }
            Event::Queue {
                pane_id,
                items,
                paused,
            } => {
                self.apply_queue(&pane_id, items, paused);
                Vec::new()
            }
            Event::Opened { ticket, pane_id } => self.opened(ticket, pane_id),
            Event::WorkspaceOpened { ticket, workspace } => {
                self.workspace_opened(ticket, workspace)
            }
            Event::OpenFailed { ticket } => {
                self.fail_request(ticket);
                Vec::new()
            }
            Event::LayoutRejected { workspace_id } => {
                self.restore_confirmed_layout(&workspace_id);
                Vec::new()
            }
            Event::Usage(readings) => {
                self.usage = readings;
                Vec::new()
            }
            Event::Harnesses(items) => {
                self.harnesses = items;
                self.harnesses_loaded = true;
                Vec::new()
            }
            Event::Catalogue { harness, catalogue } => {
                self.catalogue_failures.remove(&harness);
                self.catalogues.insert(harness, catalogue);
                Vec::new()
            }
            Event::CatalogueFailed { harness, reason } => {
                self.catalogue_failures.insert(harness, reason);
                Vec::new()
            }
            Event::Skills {
                workspace_id,
                listing,
            } => {
                self.skills_listed(workspace_id, listing);
                Vec::new()
            }
            Event::PaneCommands { pane_id, items } => {
                self.commands_listed(pane_id, items);
                Vec::new()
            }
            Event::Readiness(readiness) => {
                self.readiness = Some(readiness);
                Vec::new()
            }
            Event::Preferences(preferences) => {
                self.preferences = preferences;
                Vec::new()
            }
            Event::Editor(event) => self.editor_event(event),
            Event::Writing(event) => self.writing_event(event),
        }
    }

    fn adopt(&mut self, snapshot: Snapshot) -> Vec<Effect> {
        for workspace in &snapshot.workspaces {
            self.check_layout(workspace);
            self.confirm_layout(workspace);
        }
        self.workspaces = snapshot.workspaces;
        self.panes = snapshot.panes;
        self.forget_requests();
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
        effects.extend(self.adopt_slash_sources());
        self.adopt_loops(&snapshot.loops);
        self.adopt_tasks(&snapshot.tasks, &snapshot.attention);
        effects.extend(self.editors.adopt(snapshot.files, &self.panes));
        self.writings.adopt(snapshot.writing);
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
                let created = serde_json::from_value::<PaneCreatedEvent>(params)?;
                let mut effects = self.upsert_pane(created.pane);
                effects.extend(self.document_created(created.file, created.writing_document));
                effects
            }
            EventName::WritingChanged => self.writing_changed(params)?,
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
            _ if Self::handles_editor_event(&name) => self.editor_notification(&name, params)?,
            _ => self.agent_notification(name, params)?,
        })
    }

    fn close_workspace(&mut self, workspace_id: &str) {
        let order: Vec<String> = self
            .workspaces
            .iter()
            .map(|w| w.workspace_id.clone())
            .collect();
        self.workspaces.retain(|w| w.workspace_id != workspace_id);
        self.confirmed_layouts.remove(workspace_id);
        self.skills.remove(workspace_id);
        self.editors.workspace_closed(workspace_id);
        if self.selected_workspace.as_deref() == Some(workspace_id) {
            self.selected_workspace = workspaces::selection_after_closing(&order, workspace_id);
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
        self.confirm_layout(&workspace);
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
        if arrived {
            self.wait_for_answer(&pane);
            effects.extend(self.editors.pane_arrived(&pane));
        }
        if pane.mode == PaneMode::Agent && !self.agents.contains_key(&pane.pane_id) {
            self.agents
                .insert(pane.pane_id.clone(), AgentView::default());
            effects.push(Effect::LoadTranscript(pane.pane_id.clone()));
            effects.extend(self.slash_sources_for(&pane));
        }
        let new_terminal = is_terminal(&pane) && !self.screens.contains_key(&pane.pane_id);
        if new_terminal {
            self.screens.insert(pane.pane_id.clone(), screen_for(&pane));
            effects.push(Effect::SubscribeTerminal(pane.pane_id.clone()));
        }
        if let Some(view) = self.agents.get_mut(&pane.pane_id) {
            crate::agents::observe_turn(view, &pane);
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
        let closing = self
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .filter(|pane| pane.mode == PaneMode::Editor)
            .map(|pane| pane.workspace_id.clone());
        self.panes.retain(|pane| pane.pane_id != pane_id);
        self.screens.remove(pane_id);
        self.agents.remove(pane_id);
        self.commands.remove(pane_id);
        self.editors.pane_closed(pane_id);
        self.writings.forget_pane(pane_id);
        // The last tab closing leaves the empty editor place, which has no file to explain.
        if let Some(workspace_id) = closing.filter(|workspace_id| {
            !self
                .panes
                .iter()
                .any(|pane| pane.mode == PaneMode::Editor && pane.workspace_id == *workspace_id)
        }) {
            self.editors.clear_refusal(&workspace_id);
        }
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

mod editor;
mod layout_draft;
mod writing;
pub(crate) use layout_draft::same_panes;
mod workspaces;
pub use workspaces::{
    FolderOpening, FolderPathProblem, RecentFolder, Started, name_of, submission,
};
#[cfg(test)]
mod tests;
