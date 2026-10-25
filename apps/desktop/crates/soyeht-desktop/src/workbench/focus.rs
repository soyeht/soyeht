//! Which pane has the keyboard, and moving it.

use super::Workbench;
use crate::{composer_field::ComposerField, destination::Destination};
use gpui::{Context, Entity, FocusHandle, Focusable, Window};
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
        let form_open = self.form_open();
        match grant(&mut self.field_request, &mut self.focus_request, form_open) {
            Some(Grant::Field(handle)) => window.focus(&handle, cx),
            Some(Grant::Pane(pane_id)) => self.focus_input(&pane_id, window, cx),
            None => {}
        }
    }

    /// Asks that `field`, the first of a sheet or form that has just opened, take the keyboard on
    /// the next frame. Every form opens this way, so that its first field never waits for a click.
    pub(crate) fn focus_field_when_drawn(
        &mut self,
        field: &Entity<ComposerField>,
        cx: &mut Context<Self>,
    ) {
        self.field_request = Some(field.focus_handle(cx));
    }

    /// A sheet or form is on the stage and owns the keyboard: the new-task sheet, the
    /// new-document form of the place on the stage, or the "Start with an agent" form. A pane
    /// that asks for the keyboard meanwhile (a terminal that finished opening, say) waits.
    fn form_open(&self) -> bool {
        self.start.as_ref().is_some_and(|start| start.shown)
            || (self.destination == Destination::Tasks && self.tasks_ui.form.is_some())
            || self.places.form_open_in(self.destination)
    }

    pub(crate) fn announce_focus(&mut self) {
        if let Some(pane_id) = self.state.focused_pane().map(|pane| pane.pane_id.clone()) {
            self.send(Command::Focus { pane_id });
        }
    }
}

/// Who takes the keyboard at the start of a frame.
#[derive(Debug, PartialEq, Eq)]
enum Grant<F, P> {
    /// The first field of a form that just opened.
    Field(F),
    /// The pane that was asked for.
    Pane(P),
}

/// Settles the requests of a frame. The field of a form that just opened wins over a pane's
/// request of the same frame, which used to override it. While a form is open a pane's request
/// is kept, not granted: it takes the keyboard when the form closes.
fn grant<F, P>(
    field: &mut Option<F>,
    pane: &mut Option<P>,
    form_open: bool,
) -> Option<Grant<F, P>> {
    if let Some(field) = field.take() {
        return Some(Grant::Field(field));
    }
    if form_open {
        return None;
    }
    pane.take().map(Grant::Pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_that_just_opened_takes_the_keyboard_before_a_pane_of_the_same_frame() {
        let (mut field, mut pane) = (Some("title"), Some("terminal"));
        assert_eq!(
            grant(&mut field, &mut pane, true),
            Some(Grant::Field("title"))
        );
        assert_eq!(field, None);
        // The pane's request waits for the form to close.
        assert_eq!(pane, Some("terminal"));
    }

    #[test]
    fn a_pane_asking_while_a_form_is_open_does_not_take_the_keyboard() {
        let (mut field, mut pane): (Option<&str>, _) = (None, Some("terminal"));
        assert_eq!(grant(&mut field, &mut pane, true), None);
        assert_eq!(pane, Some("terminal"));
    }

    #[test]
    fn the_waiting_pane_takes_the_keyboard_once_the_form_is_gone() {
        let (mut field, mut pane): (Option<&str>, _) = (None, Some("terminal"));
        assert_eq!(
            grant(&mut field, &mut pane, false),
            Some(Grant::Pane("terminal"))
        );
        assert_eq!(pane, None);
    }

    #[test]
    fn with_nothing_asked_nothing_moves() {
        let (mut field, mut pane): (Option<&str>, Option<&str>) = (None, None);
        assert_eq!(grant(&mut field, &mut pane, false), None);
    }
}
