//! The parts of the replica that belong to agents and arrangement: transcripts, permission
//! requests, usage, harnesses, and where a pane that was just split goes.

use crate::{
    agent::{self, AgentView},
    layout,
    state::{Connection, Effect, Expected, State},
    switch::{HarnessSwitchView, Phase},
};
use serde_json::Value;
use soyeht_protocol::{
    EventName, HarnessInfo, Pane, PaneConfigChangedEvent, PaneHarnessSwitchDoneEvent, PaneMode,
    PanePermissionAnsweredEvent, PanePermissionRequestedEvent, PaneSessionLostEvent,
    PaneSessionStartedEvent, PaneState, PaneUpdateEvent, PermissionRequest, UsageChangedEvent,
    UsageReading, Workspace, WorkspacePaneLayoutAxis, WorkspacePaneLayoutEnvelope,
    WorkspacePaneLayoutNode,
};

impl State {
    pub fn agent(&self, pane_id: &str) -> Option<&AgentView> {
        self.agents.get(pane_id)
    }

    /// How many requests agents wait on, in `workspace` or in all of them, as the badges of the rail
    /// and the tabs count them. Nothing waits while the engine is out of reach.
    pub fn waiting_requests(&self, workspace: Option<&str>) -> usize {
        if self.connection != Connection::Connected {
            return 0;
        }
        self.agent_panes(workspace)
            .filter(|pane| {
                self.agents
                    .get(&pane.pane_id)
                    .is_some_and(|view| view.permission.is_some())
            })
            .count()
    }

    /// Whether an agent of `workspace`, or of any, is working: the dot beside the badges.
    pub fn has_working_agent(&self, workspace: Option<&str>) -> bool {
        self.connection == Connection::Connected
            && self
                .agent_panes(workspace)
                .any(|pane| pane.state == PaneState::Working)
    }

    /// Whether any agent is in the middle of a turn, so that the time in its header must tick.
    pub fn has_running_turn(&self) -> bool {
        self.agent_panes(None)
            .any(|pane| pane.state.is_turn_running())
    }

    fn agent_panes<'a>(&'a self, workspace: Option<&'a str>) -> impl Iterator<Item = &'a Pane> {
        self.panes.iter().filter(move |pane| {
            pane.mode == PaneMode::Agent && workspace.is_none_or(|id| pane.workspace_id == id)
        })
    }

    pub fn usage(&self) -> &[UsageReading] {
        &self.usage
    }

    pub fn harnesses(&self) -> &[HarnessInfo] {
        &self.harnesses
    }

    /// The display name of a harness the engine lists.
    pub fn harness_name(&self, id: &str) -> Option<&str> {
        self.harnesses
            .iter()
            .find(|harness| harness.id == id)
            .map(|harness| harness.name.as_str())
    }

    /// The arrangement to draw for the panes of a workspace that `shows`.
    pub fn layout(
        &self,
        workspace_id: &str,
        shows: impl Fn(&Pane) -> bool,
    ) -> Option<WorkspacePaneLayoutNode> {
        let workspace = self.workspace(workspace_id)?;
        let waiting = self.waiting_for_place(workspace_id);
        let panes: Vec<&Pane> = self
            .panes_of(workspace_id)
            .filter(|pane| shows(pane) && !waiting.contains(&pane.pane_id.as_str()))
            .filter(|pane| {
                // Every editor pane of the workspace shares one slot, which shows the active tab.
                let selected = self.focused_pane().map(|focused| focused.pane_id.as_str());
                self.editors.shows(pane, selected, &self.panes)
            })
            .collect();
        layout::resolve(&workspace.layout, &panes, &self.sizing)
    }

    /// The panes of `workspace_id` that wait to be told whether they are the one a split is
    /// placing: they arrived while a split request had no answer yet.
    fn waiting_for_place(&self, workspace_id: &str) -> Vec<&str> {
        self.expected
            .iter()
            .filter(|expected| {
                expected.workspace_id == workspace_id
                    && expected.split.is_some()
                    && expected.pane_id.is_none()
            })
            .flat_map(|expected| expected.arrivals.iter().map(String::as_str))
            .collect()
    }

    /// A pane arrived: when a split request is still unanswered in its workspace, it may be the
    /// pane that request opened, so it is kept out of the drawing until the answer arrives.
    pub(crate) fn wait_for_answer(&mut self, pane: &Pane) {
        if let Some(expected) = self.expected.iter_mut().find(|expected| {
            expected.workspace_id == pane.workspace_id
                && expected.split.is_some()
                && expected.pane_id.is_none()
        }) {
            expected.arrivals.push(pane.pane_id.clone());
        }
    }

    /// Remembers that the engine was asked (by request `ticket`) to open a pane in a workspace.
    pub fn expect_open(&mut self, ticket: u64, workspace_id: &str) {
        self.expected.push(Expected {
            ticket,
            workspace_id: workspace_id.to_owned(),
            split: None,
            pane_id: None,
            arrivals: Vec::new(),
        });
    }

    /// As [`State::expect_open`], for a pane that goes beside `anchor`. Returns the workspace to
    /// open it in, or `None` when `anchor` is not a pane of this replica.
    pub fn expect_split(
        &mut self,
        ticket: u64,
        anchor: &str,
        axis: WorkspacePaneLayoutAxis,
    ) -> Option<String> {
        let workspace_id = self
            .panes
            .iter()
            .find(|pane| pane.pane_id == anchor)?
            .workspace_id
            .clone();
        self.expect_open(ticket, &workspace_id);
        if let Some(expected) = self.expected.last_mut() {
            expected.split = Some((anchor.to_owned(), axis));
        }
        Some(workspace_id)
    }

    /// The engine answered request `ticket` with the pane it opened.
    pub(crate) fn opened(&mut self, ticket: u64, pane_id: String) -> Vec<Effect> {
        let Some(expected) = self
            .expected
            .iter_mut()
            .find(|expected| expected.ticket == ticket)
        else {
            return Vec::new();
        };
        expected.pane_id = Some(pane_id.clone());
        // The notification may have come first: the pane is here already.
        if self.panes.iter().any(|pane| pane.pane_id == pane_id) {
            self.settle(&pane_id)
        } else {
            Vec::new()
        }
    }

    /// `pane_id` is here: when it is the pane a request opened, it takes the keyboard and, for a
    /// split, its place beside the pane it was split from.
    pub(crate) fn settle(&mut self, pane_id: &str) -> Vec<Effect> {
        let Some(at) = self
            .expected
            .iter()
            .position(|expected| expected.pane_id.as_deref() == Some(pane_id))
        else {
            return Vec::new();
        };
        let expected = self.expected.remove(at);
        let mut effects = vec![Effect::FocusPane(pane_id.to_owned())];
        let (Some((anchor, axis)), Some(pane)) = (expected.split, self.pane(pane_id).cloned())
        else {
            return effects;
        };
        // The arrangement without the new pane, which the split then places.
        let Some(tree) = self.layout(&expected.workspace_id, |other| other.pane_id != pane_id)
        else {
            return effects;
        };
        if let Some(placed) = layout::split_pane(&tree, &anchor, &pane, axis) {
            let envelope = layout::envelope(placed);
            // Draw it at once: the engine's echo of the stored layout comes a round trip later,
            // and until then the new pane would show in the automatic place.
            if let (Some(workspace), Ok(stored)) = (
                self.workspaces
                    .iter_mut()
                    .find(|workspace| workspace.workspace_id == expected.workspace_id),
                serde_json::to_value(&envelope),
            ) {
                workspace.layout = stored;
            }
            effects.push(Effect::StoreLayout {
                workspace_id: expected.workspace_id,
                layout: envelope,
            });
        }
        effects
    }

    /// Says so when a stored layout cannot be read: the panes are tiled automatically then, and
    /// the person should know why their arrangement is not shown.
    pub(crate) fn check_layout(&mut self, workspace: &Workspace) {
        if workspace.layout.get("version").is_some()
            && serde_json::from_value::<WorkspacePaneLayoutEnvelope>(workspace.layout.clone())
                .is_err()
        {
            self.problem = Some(format!(
                "The layout of {} could not be read, so its panes are tiled automatically.",
                workspace.name
            ));
        }
    }

    /// After a snapshot: a view for every agent pane, the permission requests the engine lists,
    /// and the loads the views need.
    pub(crate) fn adopt_agents(&mut self, requests: &[Value]) -> Vec<Effect> {
        let ids: Vec<String> = self
            .panes
            .iter()
            .filter(|pane| pane.mode == PaneMode::Agent)
            .map(|pane| pane.pane_id.clone())
            .collect();
        self.agents.retain(|id, _| ids.contains(id));
        let mut effects = vec![Effect::LoadUsage, Effect::LoadHarnesses];
        for id in ids {
            let view = self.agents.entry(id.clone()).or_default();
            view.permission = None;
            effects.push(Effect::LoadTranscript(id));
        }
        for pane in &self.panes {
            if let Some(view) = self.agents.get_mut(&pane.pane_id) {
                observe_turn(view, pane);
                view.cards.adopt(pane);
                // A switch that was running when the connection began is still running.
                view.harness_switch = pane
                    .agent
                    .as_ref()
                    .and_then(|agent| agent.harness_switch.as_ref())
                    .map(|running| HarnessSwitchView {
                        from: running.from_harness.clone(),
                        to: running.to_harness.clone(),
                        phase: Phase::Switching,
                        reason: None,
                    });
            }
        }
        for entry in requests {
            match entry.get("kind").and_then(Value::as_str) {
                Some("permission") => {}
                Some("elicitation") => {
                    self.adopt_elicitation(entry);
                    continue;
                }
                _ => continue,
            }
            let request = match entry
                .get("request")
                .map(|value| serde_json::from_value::<PermissionRequest>(value.clone()))
            {
                Some(Ok(request)) => request,
                Some(Err(error)) => {
                    self.problem = Some(format!(
                        "An agent's request for permission could not be read ({error}); answer it from another client."
                    ));
                    continue;
                }
                None => continue,
            };
            if let Some(view) = self.agents.get_mut(&request.pane_id) {
                view.permission = Some(request);
            }
        }
        effects
    }

    /// Events about agents, usage and layout. Unknown names change nothing.
    pub(crate) fn agent_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            EventName::PaneUpdate => {
                if let Some((pane_id, message)) =
                    agent::message_of(serde_json::from_value::<PaneUpdateEvent>(params)?)
                    && let Some(view) = self.agents.get_mut(&pane_id)
                    && let Err(error) = view.merge(message)
                {
                    self.problem = Some(error.to_string());
                }
                Vec::new()
            }
            EventName::PaneTurnEnded => self.reload_transcript(&params),
            EventName::PaneHarnessSwitchDone => {
                let event = serde_json::from_value::<PaneHarnessSwitchDoneEvent>(params.clone())?;
                self.record_switch(
                    &event.pane_id,
                    event.from_harness,
                    event.to_harness,
                    Phase::Restarted,
                );
                self.clear_sign_in(&event.pane_id);
                if let Some(view) = self.agents.get_mut(&event.pane_id) {
                    view.notice = None;
                }
                self.reload_transcript(&params)
            }
            EventName::PaneSessionStarted => {
                let event = serde_json::from_value::<PaneSessionStartedEvent>(params.clone())?;
                if let Some(agent) = self.agent_session_mut(&event.pane_id) {
                    agent.options = event.options;
                    agent.operation = None;
                }
                if let Some(view) = self.agents.get_mut(&event.pane_id) {
                    view.notice = None;
                }
                let mut effects = self.reload_transcript(&params);
                effects.push(Effect::LoadHarnesses);
                effects
            }
            EventName::PaneConfigChanged => {
                let event = serde_json::from_value::<PaneConfigChangedEvent>(params)?;
                if let Some(agent) = self.agent_session_mut(&event.pane_id) {
                    agent.options = event.options;
                    agent.operation = None;
                }
                Vec::new()
            }
            EventName::PaneSessionLost => {
                let event = serde_json::from_value::<PaneSessionLostEvent>(params)?;
                self.set_notice(&event.pane_id, event.reason);
                Vec::new()
            }
            EventName::PanePermissionRequested => {
                let event = serde_json::from_value::<PanePermissionRequestedEvent>(params)?;
                let request = PermissionRequest {
                    request_id: event.request_id,
                    pane_id: event.pane_id.clone(),
                    generation: event.generation,
                    prompt_id: event.prompt_id,
                    title: event.title,
                    detail: event.detail,
                    presentation: event.presentation,
                    options: event.options,
                    expires_at: event.expires_at,
                    kind: event.kind,
                    domain: event.domain,
                };
                if let Some(view) = self.agents.get_mut(&event.pane_id) {
                    view.permission = Some(request);
                }
                Vec::new()
            }
            EventName::PanePermissionAnswered => {
                let event = serde_json::from_value::<PanePermissionAnsweredEvent>(params)?;
                if let Some(view) = self.agents.get_mut(&event.pane_id)
                    && view
                        .permission
                        .as_ref()
                        .is_some_and(|request| request.request_id == event.request_id)
                {
                    view.permission = None;
                }
                Vec::new()
            }
            EventName::UsageChanged => {
                self.record_usage(serde_json::from_value(params)?);
                Vec::new()
            }
            EventName::LoopTick | EventName::LoopStopped => self.loop_notification(name, params)?,
            EventName::TaskCreated
            | EventName::TaskPhraseChanged
            | EventName::TaskCommented
            | EventName::TaskSubitemChanged
            | EventName::TaskMoved
            | EventName::TaskClaimed
            | EventName::TaskReleased
            | EventName::TaskBlocked
            | EventName::TaskUnblocked
            | EventName::TaskUpdated
            | EventName::AttentionChanged => self.task_notification(name, params)?,
            _ => self.card_notification(name, params)?,
        })
    }

    pub(crate) fn reload_transcript(&self, params: &Value) -> Vec<Effect> {
        params
            .get("paneId")
            .and_then(Value::as_str)
            .filter(|id| self.agents.contains_key(*id))
            .map(|id| vec![Effect::LoadTranscript(id.to_owned())])
            .unwrap_or_default()
    }

    pub(crate) fn agent_session_mut(
        &mut self,
        pane_id: &str,
    ) -> Option<&mut soyeht_protocol::AgentSession> {
        self.panes
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)?
            .agent
            .as_mut()
    }

    /// Says why an agent's session is not usable.
    pub(crate) fn set_notice(&mut self, pane_id: &str, reason: String) {
        if let Some(view) = self.agents.get_mut(pane_id) {
            view.notice = Some(reason);
        }
    }

    fn record_usage(&mut self, event: UsageChangedEvent) {
        let reading = UsageReading {
            provider: event.provider,
            state: event.state,
            unavailable_reason: event.unavailable_reason,
            primary: event.primary,
            secondary: event.secondary,
            source: event.source,
            fetched_at: event.fetched_at,
        };
        match self
            .usage
            .iter_mut()
            .find(|known| known.provider == reading.provider)
        {
            Some(known) => *known = reading,
            None => self.usage.push(reading),
        }
    }
}

/// Tells an agent's clock which prompt its pane is answering.
pub(crate) fn observe_turn(view: &mut AgentView, pane: &Pane) {
    let prompt = pane
        .agent
        .as_ref()
        .and_then(|agent| agent.active_prompt_id.as_deref());
    view.turn.observe(prompt, std::time::Instant::now());
}

#[cfg(test)]
mod tests;
