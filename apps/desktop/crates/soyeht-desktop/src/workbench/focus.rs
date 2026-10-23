//! Which pane has the keyboard, and moving it.

use super::Workbench;
use gpui::{Context, FocusHandle, Focusable, Window};
use soyeht_client::Command;
use soyeht_protocol::PaneMode;

impl Workbench {
    pub(crate) fn focus_pane(
        &mut self,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.focus_pane(pane_id);
        self.announce_focus();
        self.menus.close();
        self.stage.reveal = Some(pane_id.to_owned());
        self.focus_input(pane_id, window, cx);
        cx.notify();
    }

    /// Moves keyboard focus to where `pane_id` takes its input: the composer of an agent, the
    /// window for a terminal.
    pub(crate) fn focus_input(
        &mut self,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = self.input_focus(pane_id, cx);
        window.focus(&handle, cx);
    }

    pub(crate) fn input_focus(&mut self, pane_id: &str, cx: &mut Context<Self>) -> FocusHandle {
        if let Some(text) = self.writing_focus(pane_id, cx) {
            return text;
        }
        let is_agent = self
            .state
            .pane(pane_id)
            .is_some_and(|pane| pane.mode == PaneMode::Agent);
        if !is_agent {
            return self.focus.clone();
        }
        self.composer_field(pane_id, cx).focus_handle(cx)
    }

    /// Called at the start of a frame: honours a request to move keyboard focus.
    pub(crate) fn apply_focus_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_start(window, cx);
        self.focus_place_form(window, cx);
        if let Some(pane_id) = self.focus_request.take() {
            self.focus_input(&pane_id, window, cx);
        }
    }

    pub(crate) fn announce_focus(&mut self) {
        if let Some(pane_id) = self.state.focused_pane().map(|pane| pane.pane_id.clone()) {
            self.send(Command::Focus { pane_id });
        }
    }
}
