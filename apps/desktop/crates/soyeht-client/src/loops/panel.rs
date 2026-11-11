//! The loop panel of an agent pane: what the person has edited and not applied yet, and what
//! applying it sends. The words and the rules around it are in the parent module.

use super::{Form, can_apply};
use crate::Command;
use soyeht_protocol::{AgentPaneRole, LoopConfig, LoopStopWhen, Pane};

/// The option whose choices the panel lists under its options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Role,
    Interval,
    Rounds,
    Stop,
}

/// A panel being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub form: Form,
    pub role: AgentPaneRole,
    /// The option whose choices are listed now.
    pub choice: Option<Choice>,
}

impl Panel {
    /// The panel of `pane` as it opens: its loop (else the defaults) and the role of its agent.
    pub fn open(pane: &Pane, config: Option<&LoopConfig>) -> Self {
        Self {
            form: Form::initial(config),
            role: pane
                .agent
                .as_ref()
                .map_or(AgentPaneRole::Worker, |agent| agent.role.clone()),
            choice: None,
        }
    }

    pub fn toggle_enabled(&mut self) {
        self.form.enabled = !self.form.enabled;
    }

    /// Lists the choices of `choice`, or closes the list when it is the one listed.
    pub fn show_choices(&mut self, choice: Choice) {
        self.choice = (self.choice != Some(choice)).then_some(choice);
    }

    /// Takes a change of one option, which closes the list it was chosen from.
    fn edit(&mut self, change: impl FnOnce(&mut Self)) {
        change(self);
        self.choice = None;
    }

    pub fn pick_interval(&mut self, seconds: u32) {
        self.edit(|panel| panel.form.interval = seconds);
    }

    pub fn pick_rounds(&mut self, rounds: u32) {
        self.edit(|panel| panel.form.max_rounds = rounds);
    }

    pub fn pick_stop(&mut self, stop_when: LoopStopWhen) {
        self.edit(|panel| panel.form.stop_when = stop_when);
    }

    pub fn pick_role(&mut self, role: AgentPaneRole) {
        self.edit(|panel| panel.role = role);
    }

    /// The requests that apply the panel to `pane` with `prompt` as the message of the loop: the
    /// pane's role when it changed, then its loop. `None` while the panel cannot be applied (a
    /// loop that is on needs a prompt).
    pub fn apply(&self, pane: &Pane, prompt: &str) -> Option<Vec<Command>> {
        if !can_apply(self.form.enabled, prompt) {
            return None;
        }
        let mut requests = Vec::new();
        let current = pane.agent.as_ref().map(|agent| &agent.role);
        if current != Some(&self.role) {
            requests.push(Command::SetRole {
                pane_id: pane.pane_id.clone(),
                role: self.role.clone(),
            });
        }
        requests.push(Command::SetLoop {
            pane_id: pane.pane_id.clone(),
            prompt: prompt.to_owned(),
            interval: self.form.interval,
            max_rounds: self.form.max_rounds,
            stop_when: self.form.stop_when.clone(),
            enabled: self.form.enabled,
        });
        Some(requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{agent_pane, pane};
    use serde_json::json;

    fn chief_pane() -> Pane {
        agent_pane("a", "w", json!({"role": "chief"}))
    }

    #[test]
    fn the_panel_opens_on_the_defaults_and_the_role_of_the_agent() {
        let panel = Panel::open(&chief_pane(), None);
        assert_eq!(panel.form, Form::initial(None));
        assert_eq!(panel.role, AgentPaneRole::Chief);
        assert_eq!(panel.choice, None);
        let terminal = Panel::open(&pane("t", "w", "terminal"), None);
        assert_eq!(terminal.role, AgentPaneRole::Worker);
    }

    #[test]
    fn the_list_of_choices_opens_closes_and_closes_when_a_choice_is_made() {
        let mut panel = Panel::open(&chief_pane(), None);
        panel.show_choices(Choice::Interval);
        assert_eq!(panel.choice, Some(Choice::Interval));
        panel.show_choices(Choice::Rounds);
        assert_eq!(panel.choice, Some(Choice::Rounds));
        panel.show_choices(Choice::Rounds);
        assert_eq!(panel.choice, None);
        panel.show_choices(Choice::Stop);
        panel.pick_stop(LoopStopWhen::ExplicitOff);
        assert_eq!(
            (panel.choice, panel.form.stop_when.clone()),
            (None, LoopStopWhen::ExplicitOff)
        );
        panel.pick_interval(5);
        panel.pick_rounds(25);
        panel.toggle_enabled();
        assert_eq!(
            (
                panel.form.interval,
                panel.form.max_rounds,
                panel.form.enabled
            ),
            (5, 25, true)
        );
    }

    #[test]
    fn applying_sends_the_role_only_when_it_changed_and_then_the_loop() {
        let pane = chief_pane();
        let mut panel = Panel::open(&pane, None);
        panel.toggle_enabled();
        let loop_request = Command::SetLoop {
            pane_id: "a".into(),
            prompt: "go".into(),
            interval: 60,
            max_rounds: 10,
            stop_when: LoopStopWhen::ThreeEmpty,
            enabled: true,
        };
        assert_eq!(panel.apply(&pane, "go"), Some(vec![loop_request.clone()]));
        panel.pick_role(AgentPaneRole::Worker);
        assert_eq!(
            panel.apply(&pane, "go"),
            Some(vec![
                Command::SetRole {
                    pane_id: "a".into(),
                    role: AgentPaneRole::Worker,
                },
                loop_request
            ])
        );
    }

    #[test]
    fn a_loop_that_is_on_needs_a_prompt_and_one_that_is_off_does_not() {
        let pane = chief_pane();
        let mut panel = Panel::open(&pane, None);
        assert!(panel.apply(&pane, " ").is_some());
        panel.toggle_enabled();
        assert_eq!(panel.apply(&pane, " \n"), None);
    }
}
