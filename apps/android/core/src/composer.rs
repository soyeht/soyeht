//! The controls of an agent's composer, as the iPhone's draws them: the harness the pane runs (and the
//! menu that switches it), then the model, effort and permission menus of the session. Which selector
//! is which, how harnesses are grouped and when a switch may start are `soyeht-client`'s rules, the
//! ones the Mac and the desktop follow (`contracts/vectors/harness-picker.json`, `harness-switch.json`).

use soyeht_client::{
    Command, State,
    agent::{Controls, Menu},
    harness_picker,
    state::Connection,
    switch::{self, Phase},
};
use soyeht_protocol::{HarnessInfo, client_policy};

/// One choice of a menu.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub name: String,
}

/// The model, effort or permission menu of a session.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ChoiceMenu {
    /// What the option is called, for the menu's title.
    pub title: String,
    /// The name on the chip: the current choice, else the option's title.
    pub label: String,
    pub config_id: String,
    pub current: Option<String>,
    pub choices: Vec<Choice>,
}

/// The harness chip.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessChip {
    /// The harness the pane runs ("Claude"), or the one it is switching to while a switch runs.
    pub title: String,
    pub busy: bool,
    /// The menu may open: connected, no configuration change in flight, no switch running.
    pub can_switch: bool,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct ComposerControls {
    pub harness: HarnessChip,
    pub model: Option<ChoiceMenu>,
    pub effort: Option<ChoiceMenu>,
    pub permission: Option<ChoiceMenu>,
    /// A change of configuration is being applied; the menus wait for it.
    pub configuring: bool,
}

/// A harness the pane can be switched to.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessChoice {
    pub id: String,
    pub name: String,
    /// How ready it is, which tells apart two harnesses of one name.
    pub status: String,
    /// It can start; the others are listed and cannot be chosen.
    pub available: bool,
    /// The button that confirms the switch ("Restart conversation with Codex").
    pub action: String,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessSection {
    pub title: String,
    pub items: Vec<HarnessChoice>,
}

/// The harnesses to choose from, laid out as the new-agent picker lays them out.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct HarnessChoices {
    pub title: String,
    /// What the switch does to the conversation, said where the choice is confirmed.
    pub handover: String,
    pub sections: Vec<HarnessSection>,
    pub more_title: String,
    pub more: Vec<HarnessChoice>,
    /// The engine has answered the request for the harnesses at least once.
    pub loaded: bool,
    /// What the menu says while the harnesses have not arrived, when there are none, and on its rescan
    /// entry; and the cancel of the confirmation (`x-clientPolicy`).
    pub loading_text: String,
    pub none_text: String,
    pub rescan_text: String,
    pub cancel_text: String,
}

fn menu(menu: Menu) -> ChoiceMenu {
    ChoiceMenu {
        label: menu.label().to_owned(),
        title: menu.title,
        config_id: menu.config_id,
        current: menu.current,
        choices: menu
            .choices
            .into_iter()
            .map(|(value, name)| Choice { value, name })
            .collect(),
    }
}

fn choice(harness: &HarnessInfo) -> HarnessChoice {
    HarnessChoice {
        id: harness.id.clone(),
        name: harness.name.clone(),
        status: harness_picker::status_label(harness).to_owned(),
        available: harness.available,
        action: harness_picker::switch_label(Some(harness)),
    }
}

pub(crate) fn composer(state: &State, pane_id: &str) -> Option<ComposerControls> {
    let pane = state.pane(pane_id)?;
    let connected = state.connection == Connection::Connected;
    let controls = Controls::new(pane, None, None, false, connected)?;
    let agent = pane.agent.as_ref()?;
    let view = state.agent(pane_id)?;
    let switching = view
        .harness_switch
        .as_ref()
        .filter(|view| view.phase == Phase::Switching)
        .map(|view| view.to.as_str());
    let (title, busy) = harness_picker::chip_title(
        state.harnesses(),
        &agent.harness,
        switching,
        Some(pane.name.as_str()),
    );
    Some(ComposerControls {
        harness: HarnessChip {
            title,
            busy,
            can_switch: switch::can_switch(connected, Some(agent), view.harness_switch.as_ref()),
        },
        model: controls.model.map(menu),
        effort: controls.effort.map(menu),
        permission: controls.permission.map(menu),
        configuring: controls.busy,
    })
}

pub(crate) fn harness_choices(state: &State, pane_id: &str) -> Option<HarnessChoices> {
    let running = state.pane(pane_id)?.agent.as_ref()?.harness.as_str();
    let layout = harness_picker::layout(state.harnesses(), "", Some(running));
    Some(HarnessChoices {
        title: client_policy::HARNESS_SWITCH_TITLE.to_owned(),
        handover: client_policy::HARNESS_SWITCH_HANDOVER.to_owned(),
        sections: layout
            .sections
            .into_iter()
            .map(|(group, items)| HarnessSection {
                title: group.title().to_owned(),
                items: items.into_iter().map(choice).collect(),
            })
            .collect(),
        more_title: harness_picker::MORE_TITLE.to_owned(),
        more: layout.more.into_iter().map(choice).collect(),
        loaded: state.harnesses_loaded(),
        loading_text: client_policy::HARNESSES_LOADING.to_owned(),
        none_text: client_policy::HARNESSES_NONE.to_owned(),
        rescan_text: client_policy::HARNESSES_RESCAN.to_owned(),
        cancel_text: client_policy::CANCEL.to_owned(),
    })
}

/// The request that switches a pane to `harness`, when it may be sent now: connected, no
/// configuration change in flight, no switch running, and the harness is one the engine lists as
/// able to start. A switch the views let through by mistake is refused here, never sent.
pub(crate) fn switch_command(state: &State, pane_id: &str, harness: &str) -> Option<Command> {
    let pane = state.pane(pane_id)?;
    let agent = pane.agent.as_ref()?;
    let connected = state.connection == Connection::Connected;
    let view = state.agent(pane_id)?;
    let startable = switch::startable(state.harnesses(), Some(&agent.harness), harness);
    (startable && switch::can_switch(connected, Some(agent), view.harness_switch.as_ref())).then(
        || Command::SwitchHarness {
            pane_id: pane_id.to_owned(),
            harness: harness.to_owned(),
            model: None,
            effort: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use soyeht_client::{Event, Snapshot};
    use soyeht_protocol::EventName;

    fn pane(session: &Value) -> Value {
        let mut agent = json!({
            "harness": "registry.claude-acp", "version": "1", "fingerprint": "f", "options": [],
            "authMethods": [], "queue": [], "draft": false, "role": "worker", "sessionId": "s",
        });
        for (key, value) in session.as_object().into_iter().flatten() {
            agent[key] = value.clone();
        }
        json!({
            "paneId": "a", "workspaceId": "w", "name": "Agent", "cwd": "/work", "mode": "agent",
            "state": "idle", "source": "none", "confidence": 0.0, "phrase": "", "generation": 1,
            "cols": 80, "rows": 24, "record": false, "agent": agent,
        })
    }

    fn harness(id: &str, name: &str, available: bool, state: Option<&str>) -> HarnessInfo {
        let mut value = json!({
            "id": id, "name": name, "available": available, "capabilities": [], "options": [],
            "authMethods": [],
        });
        if let Some(state) = state {
            value["setupState"] = json!(state);
            value["installed"] = json!(true);
        }
        serde_json::from_value(value).unwrap()
    }

    fn state(session: Value) -> State {
        let mut state = State::default();
        let workspace = json!({"workspaceId": "w", "name": "w", "path": "/work", "layout": {}});
        state.apply(Event::Connected(Snapshot {
            workspaces: vec![serde_json::from_value(workspace).unwrap()],
            panes: vec![serde_json::from_value(pane(&session)).unwrap()],
            ..Snapshot::default()
        }));
        state.apply(Event::Harnesses(vec![
            harness("registry.claude-acp", "Claude Agent", true, Some("ready")),
            harness("registry.codex-acp", "Codex", true, Some("ready")),
            harness("registry.gemini", "Gemini", false, None),
        ]));
        state
    }

    fn selector(id: &str, category: &str, current: &str, values: &[&str]) -> Value {
        json!({
            "id": id, "name": id, "category": category, "type": "select", "currentValue": current,
            "options": values.iter().map(|value| json!({"value": value, "name": value})).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn the_menus_are_the_sessions_selectors_by_what_each_one_is() {
        let options = json!({"options": [
            selector("model", "model", "m1", &["m1", "m2"]),
            selector("effort", "thought_level", "low", &["low", "high"]),
            selector("mode", "mode", "ask", &["ask", "auto"]),
        ]});
        let shown = composer(&state(options), "a").unwrap();
        let model = shown.model.unwrap();
        assert_eq!(
            (model.config_id.as_str(), model.label.as_str()),
            ("model", "m1")
        );
        assert_eq!(model.choices.len(), 2);
        assert_eq!(shown.effort.unwrap().label, "low");
        assert_eq!(shown.permission.unwrap().current.as_deref(), Some("ask"));
        assert!(!shown.configuring);
    }

    #[test]
    fn a_session_without_a_selector_has_no_menu_for_it() {
        let shown = composer(&state(json!({})), "a").unwrap();
        assert!(shown.model.is_none() && shown.effort.is_none() && shown.permission.is_none());
        assert!(composer(&State::default(), "missing").is_none());
    }

    #[test]
    fn the_chip_names_the_harness_the_pane_runs_by_the_catalogue() {
        let shown = composer(&state(json!({})), "a").unwrap();
        assert_eq!(shown.harness.title, "Claude");
        assert!(!shown.harness.busy && shown.harness.can_switch);
    }

    #[test]
    fn a_pane_cannot_switch_while_a_configuration_change_is_in_flight() {
        let operation = json!({"operation": {
            "operationId": "o", "configId": "model", "value": "m2", "startedAt": "2026-10-01T00:00:00Z",
        }});
        let shown = composer(&state(operation), "a").unwrap();
        assert!(shown.configuring && !shown.harness.can_switch);
    }

    #[test]
    fn the_choices_leave_out_the_running_harness_and_group_the_rest_by_how_ready_they_are() {
        let choices = harness_choices(&state(json!({})), "a").unwrap();
        let ready: Vec<_> = choices
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(ready, ["Ready"]);
        assert_eq!(choices.sections[0].items.len(), 1);
        let codex = &choices.sections[0].items[0];
        assert_eq!(codex.name, "Codex");
        assert_eq!(codex.action, "Restart conversation with Codex");
        assert_eq!(
            choices.more.len(),
            1,
            "a harness that is not installed sits under More"
        );
        assert_eq!(choices.more[0].id, "registry.gemini");
        assert!(!choices.more[0].available);
        assert_eq!(choices.more_title, "More harnesses");
        assert!(
            choices
                .handover
                .starts_with("This starts a new agent session")
        );
        assert!(choices.loaded);
        assert_eq!(
            (&choices.loading_text[..], &choices.none_text[..]),
            ("Loading agents…", "No other agents")
        );
        assert_eq!(
            (&choices.rescan_text[..], &choices.cancel_text[..]),
            ("Check agents again", "Cancel")
        );
    }

    #[test]
    fn a_switch_is_sent_to_a_harness_that_can_start_and_to_no_other() {
        let state = state(json!({}));
        assert_eq!(
            switch_command(&state, "a", "registry.codex-acp"),
            Some(Command::SwitchHarness {
                pane_id: "a".into(),
                harness: "registry.codex-acp".into(),
                model: None,
                effort: None,
            })
        );
        assert_eq!(
            switch_command(&state, "a", "registry.gemini"),
            None,
            "not installed"
        );
        assert_eq!(
            switch_command(&state, "a", "registry.claude-acp"),
            None,
            "already running"
        );
        assert_eq!(switch_command(&state, "a", "unknown"), None);
        assert_eq!(
            switch_command(&state, "missing", "registry.codex-acp"),
            None
        );
    }

    #[test]
    fn a_switch_waits_for_a_connection_and_for_a_change_in_flight() {
        let mut offline = state(json!({}));
        offline.apply(Event::Disconnected("gone".into()));
        assert_eq!(switch_command(&offline, "a", "registry.codex-acp"), None);
        let configuring = state(json!({"operation": {
            "operationId": "o", "configId": "model", "value": "m2", "startedAt": "2026-10-01T00:00:00Z",
        }}));
        assert_eq!(
            switch_command(&configuring, "a", "registry.codex-acp"),
            None
        );
    }

    /// When a pane may switch is the shared rule (`contracts/vectors/harness-switch.json`, which the
    /// Mac and the desktop client pass): here it is what the chip says.
    #[test]
    fn the_chip_follows_the_shared_switch_vector() {
        let vector: Value = serde_json::from_str(include_str!(
            "../../../../contracts/vectors/harness-switch.json"
        ))
        .unwrap();
        let cases = vector["canSwitch"].as_array().unwrap();
        assert!(cases.len() >= 8, "the vector file lost its cases");
        for case in cases {
            let session = match case["agent"].as_str().unwrap() {
                "configuring" => json!({"operation": {
                    "operationId": "o", "configId": "model", "value": "m",
                    "startedAt": "2026-10-01T00:00:00Z",
                }}),
                _ => json!({}),
            };
            let mut state = state(session);
            if let Some(phase) = case["switch"].as_str() {
                let at = |fields: Value| {
                    let mut base = json!({"eventId": "1", "at": "2026-10-01T00:00:00Z", "paneId": "a",
                        "fromHarness": "registry.claude-acp", "toHarness": "registry.codex-acp"});
                    for (key, field) in fields.as_object().into_iter().flatten() {
                        base[key] = field.clone();
                    }
                    base
                };
                let mut notify = |name, fields| {
                    state.apply(Event::Notification {
                        name,
                        params: at(fields),
                    })
                };
                notify(EventName::PaneHarnessSwitchStarted, json!({}));
                match phase {
                    "restarted" => notify(
                        EventName::PaneHarnessSwitchDone,
                        json!({"newSessionId": "s", "markerSeq": "4"}),
                    ),
                    "failed" => notify(
                        EventName::PaneHarnessSwitchFailed,
                        json!({"reason": "no", "restored": true}),
                    ),
                    _ => Vec::new(),
                };
            }
            if !case["connected"].as_bool().unwrap() {
                state.apply(Event::Disconnected("gone".into()));
            }
            // A pane with no agent session has no composer at all, so it cannot switch.
            let state = if case["agent"] == "none" {
                State::default()
            } else {
                state
            };
            let can = composer(&state, "a").is_some_and(|shown| shown.harness.can_switch);
            assert_eq!(
                can,
                case["canSwitch"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }
}
