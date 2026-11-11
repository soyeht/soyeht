//! What the harness switch popover has chosen so far: the person picks a harness, then a model and
//! an effort, then confirms. Nothing is sent until then. The draft lasts as long as the popover is
//! open; the window holds it and asks it what to send.

use super::{can_switch, startable};
use crate::{Command, State, harness_picker};
use soyeht_protocol::HarnessInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub pane_id: String,
    /// The harness to switch to, once one is chosen.
    pub harness: Option<String>,
    /// The model chosen; `None` leaves the choice to the harness ("Agent default").
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The model list is unfolded under the model field.
    pub models_open: bool,
}

impl Draft {
    /// The popover of `pane_id` has just opened: it starts on the default harness, and asks the
    /// engine for fresh readiness and for the models of that harness.
    pub fn open(state: &State, pane_id: &str) -> (Self, Vec<Command>) {
        let mut draft = Self {
            pane_id: pane_id.to_owned(),
            harness: None,
            model: None,
            effort: None,
            models_open: false,
        };
        let mut requests = vec![Command::LoadHarnesses { rescan: false }];
        requests.extend(draft.settle(state));
        (draft, requests)
    }

    /// The harness the pane runs.
    fn running<'a>(&self, state: &'a State) -> Option<&'a str> {
        state
            .pane(&self.pane_id)
            .and_then(|pane| pane.agent.as_ref())
            .map(|agent| agent.harness.as_str())
    }

    /// Chooses the default harness when the one chosen cannot start (or none is), as the Mac does
    /// each time it opens and each time the list changes. The answer asks for its models.
    pub fn settle(&mut self, state: &State) -> Option<Command> {
        let running = self.running(state);
        let chosen_can_start = self
            .harness
            .as_deref()
            .is_some_and(|id| startable(state.harnesses(), running, id));
        if chosen_can_start {
            return None;
        }
        let preferred = state.preferences().preferred_harness.as_deref();
        let first = harness_picker::default_harness(state.harnesses(), running, preferred)
            .map(|harness| harness.id.clone())?;
        self.choose_harness(state, &first)
    }

    /// The person picked `harness` in the list: the model and effort start over, and the models of
    /// the harness are asked for when it can start.
    pub fn choose_harness(&mut self, state: &State, harness: &str) -> Option<Command> {
        let available = startable(state.harnesses(), self.running(state), harness);
        self.harness = Some(harness.to_owned());
        self.model = None;
        self.effort = None;
        self.models_open = false;
        available.then(|| Command::LoadCatalogue {
            harness: harness.to_owned(),
        })
    }

    pub fn choose_model(&mut self, model: Option<String>) {
        self.model = model;
        self.models_open = false;
    }

    pub fn choose_effort(&mut self, effort: String) {
        self.effort = Some(effort);
    }

    pub fn toggle_models(&mut self) {
        self.models_open = !self.models_open;
    }

    /// The harness chosen, as the engine lists it.
    pub fn target<'a>(&self, state: &'a State) -> Option<&'a HarnessInfo> {
        let id = self.harness.as_deref()?;
        state.harnesses().iter().find(|harness| harness.id == id)
    }

    /// The request that confirms the switch: the pane restarts on the chosen harness and the
    /// conversation is handed over. `None` while the harness chosen cannot be switched to.
    pub fn confirm(&self, state: &State) -> Option<Command> {
        let pane = state
            .pane(&self.pane_id)
            .and_then(|pane| pane.agent.as_ref());
        let view = state
            .agent(&self.pane_id)
            .and_then(|view| view.harness_switch.as_ref());
        let connected = state.connection == crate::Connection::Connected;
        let target = self.target(state).filter(|target| target.available)?;
        can_switch(connected, pane, view).then(|| Command::SwitchHarness {
            pane_id: self.pane_id.clone(),
            acknowledged: state.acknowledged_package(&target.id),
            harness: target.id.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Event, Snapshot,
        switch::HarnessSwitchView,
        testing::{agent_pane, harness},
    };
    use serde_json::json;

    /// A connected state with an agent pane `a` running `claude`, and the harnesses listed.
    fn state_with(harnesses: Vec<HarnessInfo>) -> State {
        let mut state = State::default();
        state.apply(Event::Connected(Snapshot {
            workspaces: vec![crate::testing::workspace("w")],
            panes: vec![agent_pane("a", "w", json!({"harness": "claude"}))],
            ..Snapshot::default()
        }));
        state.harnesses = harnesses;
        state
    }

    fn load_catalogue(harness: &str) -> Command {
        Command::LoadCatalogue {
            harness: harness.into(),
        }
    }

    #[test]
    fn the_popover_opens_on_the_default_harness_and_asks_for_its_models() {
        let state = state_with(vec![harness("claude", true), harness("codex", true)]);
        let (draft, requests) = Draft::open(&state, "a");
        assert_eq!(draft.harness.as_deref(), Some("codex"));
        assert_eq!(
            requests,
            vec![
                Command::LoadHarnesses { rescan: false },
                load_catalogue("codex")
            ]
        );
    }

    #[test]
    fn a_harness_chosen_starts_the_model_and_effort_over() {
        let state = state_with(vec![
            harness("claude", true),
            harness("codex", true),
            harness("other", true),
        ]);
        let (mut draft, _) = Draft::open(&state, "a");
        draft.choose_model(Some("m".into()));
        draft.choose_effort("high".into());
        draft.toggle_models();
        assert!(draft.models_open);
        assert_eq!(
            draft.choose_harness(&state, "other"),
            Some(load_catalogue("other"))
        );
        assert_eq!(
            (draft.model, draft.effort, draft.models_open),
            (None, None, false)
        );
    }

    #[test]
    fn the_models_of_a_harness_that_cannot_start_are_not_asked_for() {
        let state = state_with(vec![harness("claude", true), harness("off", false)]);
        let (mut draft, _) = Draft::open(&state, "a");
        assert_eq!(draft.choose_harness(&state, "off"), None);
        assert_eq!(draft.harness.as_deref(), Some("off"));
        assert_eq!(
            draft.choose_harness(&state, "claude"),
            None,
            "it is running"
        );
    }

    #[test]
    fn a_list_that_changes_takes_the_choice_away_from_a_harness_that_can_no_longer_start() {
        let mut state = state_with(vec![harness("claude", true), harness("codex", true)]);
        let (mut draft, _) = Draft::open(&state, "a");
        assert_eq!(draft.settle(&state), None, "the choice still starts");
        state.harnesses = vec![harness("claude", true), harness("codex", false)];
        assert_eq!(draft.settle(&state), None, "no other harness can start");
        state.harnesses = vec![
            harness("claude", true),
            harness("codex", false),
            harness("third", true),
        ];
        assert_eq!(draft.settle(&state), Some(load_catalogue("third")));
        assert_eq!(draft.harness.as_deref(), Some("third"));
    }

    #[test]
    fn confirming_names_the_harness_with_the_model_and_effort_chosen() {
        let state = state_with(vec![harness("claude", true), harness("codex", true)]);
        let (mut draft, _) = Draft::open(&state, "a");
        draft.choose_model(Some("m".into()));
        draft.choose_effort("low".into());
        assert_eq!(
            draft.confirm(&state),
            Some(Command::SwitchHarness {
                pane_id: "a".into(),
                harness: "codex".into(),
                model: Some("m".into()),
                effort: Some("low".into()),
                acknowledged: None,
            })
        );
    }

    #[test]
    fn nothing_is_confirmed_while_disconnected_or_a_switch_runs_or_the_harness_is_down() {
        let mut state = state_with(vec![harness("claude", true), harness("codex", true)]);
        let (draft, _) = Draft::open(&state, "a");
        state.agents.get_mut("a").unwrap().harness_switch = Some(HarnessSwitchView {
            from: "claude".into(),
            to: "codex".into(),
            phase: crate::switch::Phase::Switching,
            reason: None,
        });
        assert_eq!(draft.confirm(&state), None, "a switch is running");
        state.agents.get_mut("a").unwrap().harness_switch = None;
        state.harnesses = vec![harness("claude", true), harness("codex", false)];
        assert_eq!(draft.confirm(&state), None, "the harness is not available");
        state.harnesses = vec![harness("claude", true), harness("codex", true)];
        state.apply(Event::Disconnected("gone".into()));
        assert_eq!(draft.confirm(&state), None, "not connected");
    }
}
