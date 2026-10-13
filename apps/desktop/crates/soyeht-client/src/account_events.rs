//! The parts of the replica about accounts, switching harness and settings: what the engine says
//! when a switch starts or fails, a configuration change fails, a setting changes, and what the lists the pickers and Preferences draw from hold.

use crate::{
    catalogue::Catalogue,
    preferences::Preferences,
    state::{Effect, State},
    switch::{HarnessSwitchView, Phase},
};
use serde_json::Value;
use soyeht_protocol::{
    EventName, PaneConfigFailedEvent, PaneHarnessSwitchFailedEvent, PaneHarnessSwitchStartedEvent,
    SettingsChangedEvent, SkillsListResult,
};

impl State {
    /// Whether the list of harnesses has arrived, so that an empty one means there are none.
    pub fn harnesses_loaded(&self) -> bool {
        self.harnesses_loaded
    }

    /// The models and options of a harness, once the engine has listed them.
    pub fn catalogue(&self, harness: &str) -> Option<&Catalogue> {
        self.catalogues.get(harness)
    }

    /// Why the models of a harness could not be read, while they cannot.
    pub fn catalogue_failure(&self, harness: &str) -> Option<&str> {
        self.catalogue_failures.get(harness).map(String::as_str)
    }

    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }

    /// The skills every harness loads, once the engine has listed them.
    pub fn skills(&self) -> Option<&SkillsListResult> {
        self.skills.as_ref()
    }

    /// Events about accounts, switching and settings. Unknown names change nothing.
    pub(crate) fn account_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            EventName::PaneHarnessSwitchStarted => {
                let event = serde_json::from_value::<PaneHarnessSwitchStartedEvent>(params)?;
                self.record_switch(
                    &event.pane_id,
                    event.from_harness,
                    event.to_harness,
                    Phase::Switching,
                );
                Vec::new()
            }
            EventName::PaneHarnessSwitchFailed => {
                let event = serde_json::from_value::<PaneHarnessSwitchFailedEvent>(params)?;
                self.record_switch(
                    &event.pane_id,
                    event.from_harness,
                    event.to_harness,
                    Phase::Failed,
                );
                if let Some(view) = self.agents.get_mut(&event.pane_id) {
                    if let Some(switch) = view.harness_switch.as_mut() {
                        switch.reason = Some(event.reason);
                    }
                    view.notice = Some(
                        if event.restored {
                            "The switch failed. The original agent is still running."
                        } else {
                            "The switch failed."
                        }
                        .to_owned(),
                    );
                }
                Vec::new()
            }
            EventName::PaneConfigFailed => {
                let event = serde_json::from_value::<PaneConfigFailedEvent>(params)?;
                self.set_notice(&event.pane_id, event.reason);
                Vec::new()
            }
            EventName::SkillsChanged if self.skills.is_some() => vec![Effect::LoadSkills],
            EventName::SettingsChanged => {
                let event = serde_json::from_value::<SettingsChangedEvent>(params)?;
                self.preferences.apply(&event.key, &event.value);
                Vec::new()
            }
            _ => Vec::new(),
        })
    }

    pub(crate) fn record_switch(&mut self, pane_id: &str, from: String, to: String, phase: Phase) {
        if let Some(view) = self.agents.get_mut(pane_id) {
            view.harness_switch = Some(HarnessSwitchView {
                from,
                to,
                phase,
                reason: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Event,
        snapshot::Snapshot,
        testing::{agent_pane, workspace},
    };
    use serde_json::json;

    fn state_with(session: Value) -> State {
        let mut state = State::default();
        state.apply(Event::Connected(Snapshot {
            workspaces: vec![workspace("w")],
            panes: vec![agent_pane("a", "w", session)],
            ..Snapshot::default()
        }));
        state
    }

    fn notify(state: &mut State, name: EventName, params: Value) -> Vec<Effect> {
        state.apply(Event::Notification { name, params })
    }

    fn event(fields: Value) -> Value {
        let mut base = json!({"eventId": "1", "at": "2026-10-01T00:00:00Z", "paneId": "a"});
        for (key, field) in fields.as_object().into_iter().flatten() {
            base[key] = field.clone();
        }
        base
    }

    #[test]
    fn a_switch_is_followed_from_its_start_to_its_end_or_failure() {
        let mut state = state_with(json!({}));
        let switch = |from: &str, to: &str| event(json!({"fromHarness": from, "toHarness": to}));
        notify(
            &mut state,
            EventName::PaneHarnessSwitchStarted,
            switch("claude", "codex"),
        );
        let started = state
            .agent("a")
            .and_then(|view| view.harness_switch.clone());
        assert_eq!(
            started.map(|view| (view.to, view.phase)),
            Some(("codex".into(), Phase::Switching))
        );

        let done = event(json!({
            "fromHarness": "claude", "toHarness": "codex", "newSessionId": "s", "markerSeq": "4",
        }));
        let effects = notify(&mut state, EventName::PaneHarnessSwitchDone, done);
        assert_eq!(effects, [Effect::LoadTranscript("a".into())]);
        assert_eq!(
            state
                .agent("a")
                .and_then(|view| view.harness_switch.as_ref().map(|v| v.phase)),
            Some(Phase::Restarted)
        );

        let failed = event(json!({
            "fromHarness": "claude", "toHarness": "codex", "reason": "Not signed in.",
            "restored": true,
        }));
        notify(&mut state, EventName::PaneHarnessSwitchFailed, failed);
        let view = state.agent("a");
        assert_eq!(
            view.and_then(|view| view.harness_switch.as_ref())
                .map(|v| (v.phase, v.reason.clone())),
            Some((Phase::Failed, Some("Not signed in.".into())))
        );
        assert_eq!(
            view.and_then(|view| view.notice.as_deref()),
            Some("The switch failed. The original agent is still running.")
        );
    }

    #[test]
    fn a_switch_that_was_running_when_the_client_connected_is_still_running() {
        let state =
            state_with(json!({"harnessSwitch": {"fromHarness": "claude", "toHarness": "codex"}}));
        let running = state
            .agent("a")
            .and_then(|view| view.harness_switch.as_ref());
        assert_eq!(running.map(|view| view.phase), Some(Phase::Switching));
    }

    #[test]
    fn settings_and_catalogues_land_where_the_pickers_read_them() {
        let mut state = state_with(json!({}));
        notify(
            &mut state,
            EventName::SettingsChanged,
            event(json!({"key": "harness.preferredId", "value": "codex"})),
        );
        assert_eq!(
            state.preferences().preferred_harness.as_deref(),
            Some("codex")
        );

        assert!(!state.harnesses_loaded());
        state.apply(Event::Harnesses(Vec::new()));
        assert!(state.harnesses_loaded());

        assert!(state.catalogue("codex").is_none());
        state.apply(Event::CatalogueFailed {
            harness: "codex".into(),
            reason: "timed out".into(),
        });
        assert_eq!(state.catalogue_failure("codex"), Some("timed out"));
        let catalogue = Catalogue {
            models: Vec::new(),
            options: Vec::new(),
            source: soyeht_protocol::ModelListSource::NotProbed,
        };
        state.apply(Event::Catalogue {
            harness: "codex".into(),
            catalogue,
        });
        assert!(state.catalogue("codex").is_some());
        assert_eq!(state.catalogue_failure("codex"), None);
    }

    #[test]
    fn a_change_to_the_skills_reloads_them_only_once_they_were_loaded() {
        let mut state = state_with(json!({}));
        let changed = || event(json!({"scope": "user"}));
        assert_eq!(notify(&mut state, EventName::SkillsChanged, changed()), []);
        let listing = serde_json::from_value(json!({"items": [], "userDirectory": "/skills"}));
        state.apply(Event::Skills(listing.unwrap()));
        assert_eq!(
            notify(&mut state, EventName::SkillsChanged, changed()),
            [Effect::LoadSkills]
        );
    }
}
