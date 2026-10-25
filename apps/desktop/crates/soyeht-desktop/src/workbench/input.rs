//! Keys, the clipboard and the wheel, for the pane in front.

use super::{Paste, Workbench};
use crate::composer_field::edit_of;
use gpui::{Context, KeyDownEvent, ScrollWheelEvent, Window};
use soyeht_client::{
    Command, Connection,
    keys::{self, Modifiers},
    screen::TerminalScreen,
    text_input::keys::Edit,
};
use soyeht_protocol::PaneMode;

impl Workbench {
    pub(crate) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
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
        let Some(pane_id) = self.focused_terminal() else {
            return;
        };
        self.paste_terminal(&pane_id, cx);
    }

    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
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
        // An editor takes its keys, the platform key among them: on the Mac that is its shortcut key.
        if let Some(pane_id) = self.focused_editor() {
            if self.editor_key(&pane_id, event, cx) {
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
        let Some(pane_id) = self.focused_terminal() else {
            return;
        };
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
        if let Some(screen) = self.state.screen_mut(pane_id)
            && screen.scroll_by(lines)
        {
            cx.notify();
        }
    }

    fn focused_terminal(&self) -> Option<String> {
        let pane = self.state.focused_pane()?;
        self.state
            .screen(&pane.pane_id)
            .map(|_| pane.pane_id.clone())
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
