//! The parts of the replica that belong to agents and arrangement: transcripts, permission
//! requests, usage, harnesses, and where a pane that was just split goes.

use crate::{
    agent::{self, AgentView},
    layout,
    state::{Connection, Effect, Expected, State},
};
use serde_json::Value;
use soyeht_protocol::{
    EventName, HarnessInfo, Pane, PaneConfigChangedEvent, PaneMode, PanePermissionAnsweredEvent,
    PanePermissionRequestedEvent, PaneSessionLostEvent, PaneSessionStartedEvent, PaneState,
    PaneUpdateEvent, PermissionRequest, UsageChangedEvent, UsageReading, Workspace,
    WorkspacePaneLayoutAxis, WorkspacePaneLayoutEnvelope, WorkspacePaneLayoutNode,
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
            .any(|pane| agent::is_running(&pane.state))
    }

    fn agent_panes<'a>(&'a self, workspace: Option<&'a str>) -> impl Iterator<Item = &'a Pane> {
        self.panes.iter().filter(move |pane| {
            pane.mode == PaneMode::Agent && workspace.is_none_or(|id| pane.workspace_id == id)
        })
    }

    /// The panes that have an agent transcript.
    pub fn agent_ids(&self) -> impl Iterator<Item = &str> {
        self.agents.keys().map(String::as_str)
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
        let workspace = self
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)?;
        let panes: Vec<&Pane> = self
            .panes_of(workspace_id)
            .filter(|pane| shows(pane))
            .collect();
        layout::resolve(&workspace.layout, &panes)
    }

    /// Remembers that the engine was asked (by request `ticket`) to open a pane in a workspace.
    pub fn expect_open(&mut self, ticket: u64, workspace_id: &str) {
        self.expected.push(Expected {
            ticket,
            workspace_id: workspace_id.to_owned(),
            split: None,
            pane_id: None,
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
        let (Some((anchor, axis)), Some(pane)) = (
            expected.split,
            self.panes
                .iter()
                .find(|pane| pane.pane_id == pane_id)
                .cloned(),
        ) else {
            return effects;
        };
        // The arrangement without the new pane, which the split then places.
        let Some(tree) = self.layout(&expected.workspace_id, |other| other.pane_id != pane_id)
        else {
            return effects;
        };
        if let Some(placed) = layout::split_pane(&tree, &anchor, &pane, axis) {
            effects.push(Effect::StoreLayout {
                workspace_id: expected.workspace_id,
                layout: layout::envelope(placed),
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
            }
        }
        for entry in requests {
            if entry.get("kind").and_then(Value::as_str) != Some("permission") {
                continue;
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
            EventName::PaneQueued | EventName::PaneTurnEnded | EventName::PaneHarnessSwitchDone => {
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
                self.reload_transcript(&params)
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
                if let Some(view) = self.agents.get_mut(&event.pane_id) {
                    view.notice = Some(event.reason);
                }
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
            _ => Vec::new(),
        })
    }

    fn reload_transcript(&self, params: &Value) -> Vec<Effect> {
        params
            .get("paneId")
            .and_then(Value::as_str)
            .filter(|id| self.agents.contains_key(*id))
            .map(|id| vec![Effect::LoadTranscript(id.to_owned())])
            .unwrap_or_default()
    }

    fn agent_session_mut(&mut self, pane_id: &str) -> Option<&mut soyeht_protocol::AgentSession> {
        self.panes
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)?
            .agent
            .as_mut()
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
mod tests {
    use super::*;
    use crate::{
        Event,
        snapshot::Snapshot,
        testing::{pane, pane_json, workspace, workspace_json},
    };
    use serde_json::json;

    fn connected(panes: &[(&str, &str)]) -> (State, Vec<Effect>) {
        let mut state = State::default();
        let effects = state.apply(Event::Connected(Snapshot {
            workspaces: vec![workspace("w")],
            panes: panes.iter().map(|(id, mode)| pane(id, "w", mode)).collect(),
            ..Snapshot::default()
        }));
        (state, effects)
    }

    fn notification(name: EventName, params: Value) -> Event {
        Event::Notification { name, params }
    }

    #[test]
    fn the_badges_count_requests_waiting_and_agents_working_per_workspace() {
        let (mut state, _) = connected(&[("a", "agent"), ("b", "agent"), ("t", "terminal")]);
        assert_eq!(state.waiting_requests(None), 0);
        assert!(!state.has_working_agent(None));
        state.agents.get_mut("a").unwrap().permission = serde_json::from_value(json!({
            "requestId": "r", "paneId": "a", "generation": 1, "promptId": "p", "title": "t",
            "detail": "", "options": [], "expiresAt": "2030-01-01T00:00:00Z"
        }))
        .ok();
        state
            .panes
            .iter_mut()
            .find(|p| p.pane_id == "b")
            .unwrap()
            .state = PaneState::Working;
        assert_eq!(state.waiting_requests(None), 1);
        assert_eq!(state.waiting_requests(Some("w")), 1);
        assert_eq!(state.waiting_requests(Some("other")), 0);
        assert!(state.has_working_agent(Some("w")) && !state.has_working_agent(Some("other")));
        state.apply(Event::Disconnected("gone".into()));
        assert_eq!(state.waiting_requests(None), 0);
        assert!(!state.has_working_agent(None));
    }

    #[test]
    fn a_snapshot_loads_every_agent_transcript_the_usage_and_the_harnesses() {
        let (state, effects) = connected(&[("a", "agent"), ("t", "terminal")]);
        assert!(state.agent("a").is_some() && state.agent("t").is_none());
        assert!(effects.contains(&Effect::LoadTranscript("a".into())));
        assert!(effects.contains(&Effect::LoadUsage) && effects.contains(&Effect::LoadHarnesses));
    }

    #[test]
    fn a_transcript_page_replaces_the_messages_and_updates_extend_them() {
        let (mut state, _) = connected(&[("a", "agent")]);
        let item = json!({"localId": "u", "generation": 1, "seq": "1", "role": "user",
                          "kind": "message", "text": "oi", "complete": true});
        state.apply(Event::Transcript {
            pane_id: "a".into(),
            items: vec![serde_json::from_value(item).unwrap()],
        });
        state.apply(notification(
            EventName::PaneUpdate,
            json!({"eventId": "9", "at": "now", "paneId": "a", "kind": "agent_message_chunk",
                   "generation": 1, "seq": "2", "localId": "m", "text": "Oi!", "complete": true}),
        ));
        let texts: Vec<_> = state
            .agent("a")
            .unwrap()
            .messages
            .iter()
            .map(|m| m.text.clone())
            .collect();
        assert_eq!(texts, ["oi", "Oi!"]);
    }

    #[test]
    fn a_finished_turn_reloads_the_transcript_of_that_agent_only() {
        let (mut state, _) = connected(&[("a", "agent"), ("t", "terminal")]);
        let ended = |pane: &str| {
            notification(
                EventName::PaneTurnEnded,
                json!({"eventId": "9", "at": "now", "paneId": pane, "promptId": "p",
                       "stopReason": "end_turn"}),
            )
        };
        assert_eq!(
            state.apply(ended("a")),
            [Effect::LoadTranscript("a".into())]
        );
        assert!(state.apply(ended("t")).is_empty());
    }

    #[test]
    fn permission_requests_wait_until_they_are_answered() {
        let (mut state, _) = connected(&[("a", "agent")]);
        state.apply(notification(
            EventName::PanePermissionRequested,
            json!({"eventId": "9", "at": "now", "paneId": "a", "requestId": "r", "generation": 1,
                   "promptId": "p", "title": "Run ls", "detail": "", "options": [],
                   "expiresAt": "later"}),
        ));
        assert_eq!(
            state.agent("a").unwrap().permission.as_ref().unwrap().title,
            "Run ls"
        );
        state.apply(notification(
            EventName::PanePermissionAnswered,
            json!({"eventId": "10", "at": "now", "paneId": "a", "requestId": "r",
                   "result": "selected", "by": "mac"}),
        ));
        assert!(state.agent("a").unwrap().permission.is_none());
    }

    #[test]
    fn usage_readings_arrive_by_list_and_change_by_event() {
        let (mut state, _) = connected(&[]);
        let reading = |used: f64| {
            json!({"provider": "claude", "state": "ok", "source": "claude_cli_usage",
                   "fetchedAt": "now",
                   "primary": {"used": used, "resetsAt": "later", "label": "Weekly"}})
        };
        state.apply(Event::Usage(vec![
            serde_json::from_value(reading(0.1)).unwrap(),
        ]));
        let mut changed = reading(0.2);
        changed["eventId"] = json!("11");
        changed["at"] = json!("now");
        state.apply(notification(EventName::UsageChanged, changed));
        assert_eq!(state.usage().len(), 1);
        assert_eq!(state.usage()[0].primary.as_ref().unwrap().used, 0.2);
    }

    fn created(id: &str) -> Event {
        notification(
            EventName::PaneCreated,
            json!({"eventId": "9", "at": "now", "pane": pane_json(id, "w", "terminal")}),
        )
    }

    /// Neither a focus request nor a stored layout: no pane was settled.
    fn opened_nothing(effects: &[Effect]) -> bool {
        !effects
            .iter()
            .any(|e| matches!(e, Effect::FocusPane(_) | Effect::StoreLayout { .. }))
    }

    fn stored(effects: &[Effect]) -> Option<WorkspacePaneLayoutNode> {
        effects.iter().find_map(|effect| match effect {
            Effect::StoreLayout { layout, .. } => layout.tree.clone(),
            _ => None,
        })
    }

    #[test]
    fn a_split_places_the_new_pane_beside_its_anchor_and_stores_the_layout() {
        let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
        assert_eq!(
            state
                .expect_split(1, "a", WorkspacePaneLayoutAxis::Vertical)
                .as_deref(),
            Some("w")
        );
        let mut effects = state.apply(created("n"));
        assert!(opened_nothing(&effects));
        effects = state.apply(Event::Opened {
            ticket: 1,
            pane_id: "n".into(),
        });
        assert!(effects.contains(&Effect::FocusPane("n".into())));
        let tree = stored(&effects).unwrap();
        assert_eq!(layout::pane_ids(&tree), ["a", "n", "b"]);
        // The request is spent: the next pane is placed by the automatic arrangement.
        let effects = state.apply(created("m"));
        assert!(opened_nothing(&effects));
        assert!(
            state
                .layout("w", |_| true)
                .is_some_and(|tree| layout::contains(&tree, "m"))
        );
    }

    #[test]
    fn the_answer_may_come_before_the_notification() {
        let (mut state, _) = connected(&[("a", "terminal")]);
        state.expect_split(2, "a", WorkspacePaneLayoutAxis::Horizontal);
        assert!(
            state
                .apply(Event::Opened {
                    ticket: 2,
                    pane_id: "n".into()
                })
                .is_empty()
        );
        let effects = state.apply(created("n"));
        assert!(effects.contains(&Effect::FocusPane("n".into())));
        assert_eq!(layout::pane_ids(&stored(&effects).unwrap()), ["a", "n"]);
    }

    #[test]
    fn a_pane_someone_else_opens_is_neither_focused_nor_placed() {
        let (mut state, _) = connected(&[("a", "terminal")]);
        state.expect_split(3, "a", WorkspacePaneLayoutAxis::Vertical);
        let effects = state.apply(created("theirs"));
        assert!(opened_nothing(&effects));
        state.apply(Event::OpenFailed { ticket: 3 });
        // The failed request no longer waits: the next pane is not placed beside "a".
        let effects = state.apply(Event::Opened {
            ticket: 3,
            pane_id: "n".into(),
        });
        assert!(effects.is_empty(), "nothing waits for ticket 3 any more");
    }

    #[test]
    fn a_connection_that_ends_drops_the_requests_in_flight() {
        let (mut state, _) = connected(&[("a", "terminal")]);
        state.expect_open(4, "w");
        state.apply(Event::Disconnected("lost".into()));
        assert!(state.expected.is_empty());
    }

    #[test]
    fn a_stored_layout_that_cannot_be_read_is_reported() {
        let mut state = State::default();
        let mut broken = workspace_json("w");
        broken["layout"] = json!({"version": 2, "mode": "sideways"});
        state.apply(notification(
            EventName::WorkspaceCreated,
            json!({"eventId": "1", "at": "now", "workspace": broken}),
        ));
        assert!(
            state
                .problem
                .as_deref()
                .is_some_and(|p| p.contains("layout"))
        );
    }

    #[test]
    fn a_permission_request_that_cannot_be_read_is_reported() {
        let mut state = State::default();
        state.apply(Event::Connected(Snapshot {
            workspaces: vec![workspace("w")],
            panes: vec![pane("a", "w", "agent")],
            requests: vec![json!({"kind": "permission", "request": {"requestId": 7}})],
            ..Snapshot::default()
        }));
        assert!(
            state
                .problem
                .as_deref()
                .is_some_and(|p| p.contains("permission"))
        );
    }

    #[test]
    fn a_transcript_page_for_a_pane_that_is_gone_is_ignored() {
        let (mut state, _) = connected(&[("a", "agent")]);
        state.apply(Event::Transcript {
            pane_id: "gone".into(),
            items: Vec::new(),
        });
        assert!(state.agent("gone").is_none());
    }

    #[test]
    fn a_layout_can_leave_out_the_panes_a_place_does_not_show() {
        let (state, _) = connected(&[("a", "terminal"), ("b", "agent")]);
        let tree = state
            .layout("w", |pane| pane.mode == PaneMode::Agent)
            .unwrap();
        assert_eq!(layout::pane_ids(&tree), ["b"]);
        assert!(state.layout("w", |_| false).is_none());
    }

    #[test]
    fn a_workspace_layout_event_changes_how_panes_are_arranged() {
        let (mut state, _) = connected(&[("a", "terminal"), ("b", "terminal")]);
        let mut changed = workspace_json("w");
        changed["layout"] = json!({"version": 2, "mode": "custom", "tree": {
            "axis": "vertical", "children": [
                {"weight": 1.0, "node": {"paneId": "b", "kind": "terminal"}},
                {"weight": 1.0, "node": {"paneId": "a", "kind": "terminal"}}]}});
        state.apply(notification(
            EventName::WorkspaceLayoutUpdated,
            json!({"eventId": "9", "at": "now", "workspace": changed}),
        ));
        let tree = state.layout("w", |_| true).unwrap();
        assert_eq!(layout::pane_ids(&tree), ["b", "a"]);
    }
}
