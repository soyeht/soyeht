//! The model the OpenCode panes use, which the OpenCode meter follows (ADR 0133). OpenCode's own
//! files only say what a new session starts with; a pane can be moved to another provider with
//! `pane.config.set`, and the meter has to show the plan of the provider the person works with.

use crate::{agents::state::State, core::Core};
use soyeht_protocol::UsageProvider;
use soyeht_usage::ReadHints;

/// The model the pane of `state` uses, when it is an OpenCode pane with a model selector. The
/// selector is the one the clients' policy calls the model (`ConfigOption::is_model`).
pub(crate) fn opencode_model(state: &State) -> Option<&str> {
    (UsageProvider::for_harness(&state.launch.id) == Some(UsageProvider::Opencode))
        .then(|| state.options.current_model())
        .flatten()
}

/// The model of the most recently active of `panes`, among those that have one.
fn latest_model<'a>(panes: impl Iterator<Item = &'a State>) -> Option<&'a str> {
    panes
        .filter_map(|state| Some((state.last_update_ms, opencode_model(state)?)))
        .max_by_key(|(updated_ms, _)| *updated_ms)
        .map(|(_, model)| model)
}

impl Core {
    /// What the readers are told about the panes, copied under the Core lock. A pane that is
    /// closed is not worked in.
    pub(crate) fn usage_hints(&self) -> ReadHints {
        let open = |pane: &String| {
            self.panes
                .get(pane)
                .is_some_and(|running| !running.stored.closed)
        };
        let panes = self
            .agents
            .iter()
            .filter(|(pane, _)| open(pane))
            .map(|(_, running)| &running.state);
        ReadHints {
            opencode_model: latest_model(panes).map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::latest_model;
    use crate::{agents::state::State, test_panes};
    use soyeht_protocol::{ConfigOption, ConfigType, ConfigValue};

    /// A harness id the clients' policy maps to the OpenCode meter.
    const OPENCODE_HARNESS_ID: &str = "registry.opencode";

    fn pane(harness: &str, option: &str, category: Option<&str>, model: &str, at: i64) -> State {
        let mut state = test_panes::ready_agent_state();
        state.launch.id = harness.into();
        state.last_update_ms = at;
        state.options.values.push(ConfigOption {
            id: option.into(),
            name: option.into(),
            description: None,
            category: category.map(str::to_owned),
            r#type: ConfigType::Select,
            current_value: ConfigValue::V0(model.into()),
            options: Vec::new(),
        });
        state
    }

    fn opencode(model: &str, at: i64) -> State {
        pane(OPENCODE_HARNESS_ID, "model", Some("model"), model, at)
    }

    #[test]
    fn the_hint_is_the_model_of_the_most_recently_active_opencode_pane() {
        let panes = [
            opencode("zai-coding-plan/glm-5.3", 10),
            opencode("opencode-go/glm-5.3-flash", 30),
            opencode("openai/gpt-5", 20),
        ];
        assert_eq!(
            latest_model(panes.iter()),
            Some("opencode-go/glm-5.3-flash")
        );
    }

    #[test]
    fn other_harnesses_and_other_selectors_are_not_the_opencode_model() {
        let panes = [
            opencode("opencode-go/glm-5.3-flash", 10),
            // More recent, but another harness's model.
            pane(
                "registry.claude-acp",
                "model",
                Some("model"),
                "claude/x",
                50,
            ),
            // More recent, but the selector is the effort one.
            pane(
                OPENCODE_HARNESS_ID,
                "effort",
                Some("thought_level"),
                "a/high",
                40,
            ),
        ];
        assert_eq!(
            latest_model(panes.iter()),
            Some("opencode-go/glm-5.3-flash")
        );
        assert_eq!(latest_model(panes[1..].iter()), None);
        assert_eq!(latest_model([].iter()), None);
    }

    #[test]
    fn the_model_selector_is_found_by_its_category_and_not_by_a_typed_name() {
        let panes = [pane(
            OPENCODE_HARNESS_ID,
            "provider-model-picker",
            Some("model"),
            "opencode-go/glm-5.3-flash",
            1,
        )];
        assert_eq!(
            latest_model(panes.iter()),
            Some("opencode-go/glm-5.3-flash")
        );
    }

    #[test]
    fn a_closed_pane_is_not_followed() {
        let (mut core, directory) = test_panes::core_with_agent(&[]);
        let state = &mut core.agents.get_mut("agent").unwrap().state;
        *state = opencode("opencode-go/glm-5.3-flash", 5);
        assert_eq!(
            core.usage_hints().opencode_model.as_deref(),
            Some("opencode-go/glm-5.3-flash")
        );
        core.panes.get_mut("agent").unwrap().stored.closed = true;
        assert_eq!(core.usage_hints().opencode_model, None);
        crate::test_paths::remove_after(core, directory);
    }
}
