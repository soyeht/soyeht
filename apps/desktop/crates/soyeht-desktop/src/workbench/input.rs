//! Keys, the clipboard and the wheel, for the pane in front.

use super::{Paste, Workbench};
use crate::{
    composer_field::{chord_with_text, edit_of},
    destination::Destination,
};
use gpui::{App, Context, KeyDownEvent, ScrollWheelEvent, Window};
use soyeht_client::{
    Command, Connection,
    editor::keys::types_text,
    keys::{self, Modifiers},
    screen::TerminalScreen,
    text_input::keys::Edit,
};
use soyeht_protocol::PaneMode;

impl Workbench {
    pub(crate) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(pane_id) = self.focused_editor() {
            if let Some(text) = item.text() {
                self.editor_paste(&pane_id, &text, cx);
            }
            return;
        }
        if let Some(pane) = self.state.focused_pane()
            && pane.mode == PaneMode::Agent
        {
            let pane_id = pane.pane_id.clone();
            self.paste_into_composer(&pane_id, &item, cx);
            return;
        }
        let Some(KeyTarget::Terminal(pane_id)) = self.keys_target(window, cx) else {
            return;
        };
        self.paste_terminal(&pane_id, cx);
    }

    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        // A name being edited takes the keys its field does not handle: they are not the pane's.
        if self.rename.is_some() {
            return;
        }
        // Escape closes an open menu, as on the Mac, and is not typed into the terminal.
        if keystroke.key == "escape"
            && (self.prefs.mode_menu.take().is_some() || self.menus.escape())
        {
            cx.notify();
            cx.stop_propagation();
            return;
        }
        // The pane in front takes the keys only while the stage shows it and no field holds the
        // keyboard (`keys_target`, the rule the typed text follows too).
        let target = self.keys_target(window, cx);
        // An editor takes its keys, the platform key among them: on the Mac that is its shortcut key.
        if let Some(KeyTarget::Editor(pane_id)) = &target {
            if self.editor_key(pane_id, event, cx) {
                cx.stop_propagation();
            }
            return;
        }
        // A copy with an agent pane in front takes what is selected in its transcript, when the
        // composer had no selection of its own; in a terminal the key is the program's.
        if self
            .state
            .focused_pane()
            .is_some_and(|pane| pane.mode == PaneMode::Agent)
        {
            if edit_of(keystroke) == Some(Edit::Copy) && self.copy_transcript(cx) {
                cx.stop_propagation();
            }
            return;
        }
        // The platform key belongs to application shortcuts, never to the terminal.
        if keystroke.modifiers.platform {
            return;
        }
        let Some(KeyTarget::Terminal(pane_id)) = target else {
            return;
        };
        // What a key types (a dead key, a letter, AltGr) comes through the platform's text input
        // (`text_input`), which composes it. The other keys are the program's.
        if types_text(&chord_with_text(keystroke), event.prefer_character_input) {
            return;
        }
        let application_cursor = self
            .state
            .screen(&pane_id)
            .is_some_and(TerminalScreen::application_cursor);
        let modifiers = Modifiers {
            control: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
        };
        if let Some(bytes) = keys::encode(
            &keystroke.key,
            keystroke.key_char.as_deref(),
            modifiers,
            application_cursor,
        ) {
            self.send_input(&pane_id, bytes, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn scroll_terminal(
        &mut self,
        pane_id: &str,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let line = crate::terminal_view::CellSize::measure(cx).height;
        let lines = event.delta.pixel_delta(line).y / line;
        // A turn down is the terminal's; the stage does not scroll with it. A sideways one goes
        // on to the stage.
        if crate::views::is_turn_down(event) {
            cx.stop_propagation();
        }
        if let Some(screen) = self.state.screen_mut(pane_id)
            && screen.scroll_by(lines)
        {
            cx.notify();
        }
    }

    /// The pane that takes the keys, the paste and the typed text: the editor or the terminal in
    /// front. It takes them only while the stage shows it: a sheet or a place in front of it, or
    /// a form that has not yet taken the keyboard, keep them, as does a field that holds the
    /// keyboard (the title of a new task, a search line). What is typed there never reaches the
    /// pane behind.
    pub(crate) fn keys_target(&self, window: &Window, cx: &App) -> Option<KeyTarget> {
        let start_shown = self.start.as_ref().is_some_and(|start| start.shown);
        let free = panes_take_keys(
            (self.destination, start_shown),
            self.field_request.is_some(),
            pane_takes_keys(window.focused(cx), self.focus.clone()),
        );
        let pane = self.state.focused_pane()?;
        match pane.mode {
            PaneMode::Editor if free => Some(KeyTarget::Editor(pane.pane_id.clone())),
            PaneMode::Terminal if free && self.state.screen(&pane.pane_id).is_some() => {
                Some(KeyTarget::Terminal(pane.pane_id.clone()))
            }
            _ => None,
        }
    }

    /// Typing returns the view to the live bottom, as in every terminal.
    pub(crate) fn send_input(&mut self, pane_id: &str, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if self.state.connection != Connection::Connected {
            // Keystrokes cannot reach a dead connection; say so instead of dropping them.
            self.state.report_disconnected();
            cx.notify();
            return;
        }
        if let Some(screen) = self.state.screen_mut(pane_id) {
            // What is typed or pasted ends a selection, as in every terminal.
            let selected = screen.has_selection();
            screen.clear_selection();
            let scrolled = screen.scrolled_back();
            if scrolled {
                screen.scroll_to_bottom();
            }
            if selected || scrolled {
                cx.notify();
            }
        }
        self.send(Command::Input {
            pane_id: pane_id.to_owned(),
            bytes,
        });
    }
}

/// Whether the keyboard is with the pane in front rather than with a field of a form: nothing
/// holds it, or the window's own handle does.
fn pane_takes_keys<H: PartialEq>(focused: Option<H>, window: H) -> bool {
    focused.is_none_or(|focused| focused == window)
}

/// The pane that takes the keys (`keys_target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyTarget {
    Terminal(String),
    Editor(String),
}

/// Whether the panes take the keys now: the stage shows them (`destination`, whether the start
/// form is shown), no form field has been asked for the keyboard and none holds it.
fn panes_take_keys(
    (destination, start_shown): (Destination, bool),
    field_requested: bool,
    keyboard_with_window: bool,
) -> bool {
    stage_shows_panes(destination, start_shown) && !field_requested && keyboard_with_window
}

/// Whether the stage draws the panes: no place of documents or tasks and no start form is in
/// front of them.
fn stage_shows_panes(destination: Destination, start_shown: bool) -> bool {
    destination.is_pane_grid() && !start_shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panes_take_keys_only_while_the_stage_shows_them() {
        assert!(stage_shows_panes(Destination::Agents, false));
        assert!(stage_shows_panes(Destination::Terminal, false));
        assert!(!stage_shows_panes(Destination::Agents, true));
        for place in [
            Destination::Tasks,
            Destination::Texts,
            Destination::Presentations,
            Destination::Screenplays,
        ] {
            assert!(!stage_shows_panes(place, false), "{place:?}");
        }
    }

    #[test]
    fn no_pane_takes_the_keys_behind_a_place_a_form_or_a_field() {
        // The Tasks place with nothing focused: what is typed is no pane's.
        assert!(!panes_take_keys((Destination::Tasks, false), false, true));
        // The frame before a form's field takes the keyboard.
        assert!(!panes_take_keys((Destination::Agents, false), true, true));
        // The start form in front of the panes.
        assert!(!panes_take_keys((Destination::Agents, true), false, true));
        // A field that holds the keyboard.
        assert!(!panes_take_keys((Destination::Agents, false), false, false));
        assert!(panes_take_keys((Destination::Editor, false), false, true));
        assert!(panes_take_keys((Destination::Terminal, false), false, true));
    }

    #[test]
    fn the_terminal_takes_the_keys_when_the_window_holds_the_keyboard() {
        assert!(pane_takes_keys(Some("window"), "window"));
        assert!(pane_takes_keys(None, "window"));
    }

    #[test]
    fn a_form_field_that_holds_the_keyboard_keeps_its_keys_from_the_terminal() {
        assert!(!pane_takes_keys(Some("title"), "window"));
    }
}
