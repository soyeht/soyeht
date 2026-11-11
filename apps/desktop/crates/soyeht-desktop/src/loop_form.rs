//! The loop panel of an agent pane: what the person has edited and not applied yet, and what
//! applying it does. The words and the rules are `soyeht_client::loops`.

use crate::composer_field::{ComposerEvent, ComposerField, new_field};
use crate::workbench::Workbench;
use gpui::{Context, Entity};
use soyeht_client::loops::{Choice, Panel};
use soyeht_protocol::{AgentPaneRole, LoopStopWhen, Pane};

/// The panel being edited and the field of its prompt.
pub(crate) struct LoopForm {
    pub panel: Panel,
    pub prompt: Entity<ComposerField>,
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
            let panel = Panel::open(pane, self.state.loop_of(&pane.pane_id));
            let prompt = new_field(self.theme.palette(), cx);
            prompt.update(cx, |field, cx| field.insert(&panel.form.prompt, cx));
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
            self.panes.of(&pane.pane_id).loop_form = Some(LoopForm { panel, prompt });
        }
    }

    pub(crate) fn toggle_loop(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, Panel::toggle_enabled);
    }

    pub(crate) fn show_choices(&mut self, pane_id: &str, choice: Choice, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |panel| panel.show_choices(choice));
    }

    pub(crate) fn pick_interval(&mut self, pane_id: &str, seconds: u32, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |panel| panel.pick_interval(seconds));
    }

    pub(crate) fn pick_rounds(&mut self, pane_id: &str, rounds: u32, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |panel| panel.pick_rounds(rounds));
    }

    pub(crate) fn pick_stop(
        &mut self,
        pane_id: &str,
        stop_when: LoopStopWhen,
        cx: &mut Context<Self>,
    ) {
        self.edit_loop(pane_id, cx, |panel| panel.pick_stop(stop_when));
    }

    pub(crate) fn pick_role(&mut self, pane_id: &str, role: AgentPaneRole, cx: &mut Context<Self>) {
        self.edit_loop(pane_id, cx, |panel| panel.pick_role(role));
    }

    fn edit_loop(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Panel),
    ) {
        if let Some(form) = self.loop_form_mut(pane_id) {
            change(&mut form.panel);
            cx.notify();
        }
    }

    /// Applies the panel: the pane's role when it changed, then its loop.
    pub(crate) fn apply_loop(&mut self, pane: &Pane, cx: &mut Context<Self>) {
        let Some(form) = self.loop_form(&pane.pane_id) else {
            return;
        };
        let prompt = form.prompt.read(cx).draft().text().to_owned();
        let Some(requests) = form.panel.apply(pane, &prompt) else {
            return;
        };
        for request in requests {
            self.send(request);
        }
        self.close_menu(cx);
    }
}
