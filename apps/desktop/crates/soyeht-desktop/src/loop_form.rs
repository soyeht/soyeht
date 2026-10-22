//! The loop panel of an agent pane: what the person has edited and not applied yet, and what
//! applying it does. The words and the rules are `soyeht_client::loops`.

use crate::composer_field::{ComposerEvent, ComposerField, new_field};
use crate::workbench::Workbench;
use gpui::{Context, Entity};
use soyeht_client::{
    Command,
    loops::{self, Form},
};
use soyeht_protocol::{AgentPaneRole, LoopStopWhen, Pane};

/// The option whose choices the panel lists under its options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Role,
    Interval,
    Rounds,
    Stop,
}

pub(crate) struct LoopForm {
    pub form: Form,
    pub role: AgentPaneRole,
    pub prompt: Entity<ComposerField>,
    pub choice: Option<Choice>,
}

impl Workbench {
    pub(crate) fn loop_form(&self, pane_id: &str) -> Option<&LoopForm> {
        self.panes.get(pane_id)?.loop_form.as_ref()
    }

    fn loop_form_mut(&mut self, pane_id: &str) -> Option<&mut LoopForm> {
        self.panes.get_mut(pane_id)?.loop_form.as_mut()
    }

    /// Makes the panel of `pane` from its loop the first time it is drawn after opening.
    pub(crate) fn ensure_loop_form(&mut self, pane: &Pane, cx: &mut Context<Self>) {
        if self.loop_form(&pane.pane_id).is_none() {
            let form = Form::initial(self.state.loop_of(&pane.pane_id));
            let role = pane
                .agent
                .as_ref()
                .map_or(AgentPaneRole::Worker, |agent| agent.role.clone());
            let prompt = new_field(self.theme.palette(), cx);
            prompt.update(cx, |field, cx| field.insert(&form.prompt, cx));
            // Return in the prompt adds a line, as in any text area.
            cx.subscribe(
                &prompt,
                |this, field, event: &ComposerEvent, cx| match event {
                    ComposerEvent::Submit => field.update(cx, |field, cx| field.insert("\n", cx)),
                    ComposerEvent::Dismiss => this.close_menu(cx),
                    // A prompt takes text only.
                    ComposerEvent::Attach => {}
                },
            )
            .detach();
            self.panes.of(&pane.pane_id).loop_form = Some(LoopForm {
                form,
                role,
                prompt,
                choice: None,
            });
        }
    }

    pub(crate) fn toggle_loop(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if let Some(form) = self.loop_form_mut(pane_id) {
            form.form.enabled = !form.form.enabled;
            cx.notify();
        }
    }

    pub(crate) fn show_choices(&mut self, pane_id: &str, choice: Choice, cx: &mut Context<Self>) {
        if let Some(form) = self.loop_form_mut(pane_id) {
            form.choice = if form.choice == Some(choice) {
                None
            } else {
                Some(choice)
            };
            cx.notify();
        }
    }

    pub(crate) fn pick_interval(&mut self, pane_id: &str, seconds: u32, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |form| form.form.interval = seconds);
    }

    pub(crate) fn pick_rounds(&mut self, pane_id: &str, rounds: u32, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |form| form.form.max_rounds = rounds);
    }

    pub(crate) fn pick_stop(
        &mut self,
        pane_id: &str,
        stop_when: LoopStopWhen,
        cx: &mut Context<Self>,
    ) {
        self.edit_loop(pane_id, cx, |form| form.form.stop_when = stop_when);
    }

    pub(crate) fn pick_role(&mut self, pane_id: &str, role: AgentPaneRole, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |form| form.role = role);
    }

    fn edit_loop(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut LoopForm),
    ) {
        if let Some(form) = self.loop_form_mut(pane_id) {
            change(form);
            form.choice = None;
            cx.notify();
        }
    }

    /// Applies the panel: the pane's role when it changed, then its loop.
    pub(crate) fn apply_loop(&mut self, pane: &Pane, cx: &mut Context<Self>) {
        let Some(form) = self.loop_form(&pane.pane_id) else {
            return;
        };
        let prompt = form.prompt.read(cx).draft().text().to_owned();
        if !loops::can_apply(form.form.enabled, &prompt) {
            return;
        }
        let (role, form) = (form.role.clone(), form.form.clone());
        let current = pane.agent.as_ref().map(|agent| agent.role.clone());
        if current.as_ref() != Some(&role) {
            self.send(Command::SetRole {
                pane_id: pane.pane_id.clone(),
                role,
            });
        }
        let (interval, max_rounds, stop_when, enabled) =
            (form.interval, form.max_rounds, form.stop_when, form.enabled);
        self.send(Command::SetLoop {
            pane_id: pane.pane_id.clone(),
            prompt,
            interval,
            max_rounds,
            stop_when,
            enabled,
        });
        self.close_menu(cx);
    }
}
