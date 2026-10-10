//! The workbench: the root view. It owns the engine connection and the client state, turns
//! engine events into state changes, and turns keyboard and pointer input into commands.
//! Rendering lives in `views`.

use crate::theme::ThemeName;
use gpui::{Context, FocusHandle, KeyDownEvent, ScrollWheelEvent, Task, Window, actions};
use soyeht_client::{
    Command, Connection, Effect, Engine, Error, Event, State,
    keys::{self, Modifiers},
    screen::TerminalScreen,
};

actions!(soyeht, [Quit, NewTerminal, Paste]);

/// Engine events applied per frame at most, so a burst of output cannot starve painting.
const EVENTS_PER_BATCH: usize = 256;

pub struct Workbench {
    engine: Engine,
    pub(crate) state: State,
    pub(crate) theme: ThemeName,
    pub(crate) focus: FocusHandle,
    _events: Task<()>,
}

impl Workbench {
    pub fn new(
        engine: Engine,
        inbox: async_channel::Receiver<Event>,
        cx: &mut Context<Self>,
    ) -> Self {
        let events = cx.spawn(async move |this, cx| {
            while let Ok(first) = inbox.recv().await {
                let mut batch = vec![first];
                while batch.len() < EVENTS_PER_BATCH {
                    match inbox.try_recv() {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                if this.update(cx, |this, cx| this.apply(batch, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            engine,
            state: State::default(),
            theme: ThemeName::DEFAULT,
            focus: cx.focus_handle(),
            _events: events,
        }
    }

    pub fn screen(&self, pane_id: &str) -> Option<&TerminalScreen> {
        self.state.screen(pane_id)
    }

    fn apply(&mut self, events: Vec<Event>, cx: &mut Context<Self>) {
        for event in events {
            for effect in self.state.apply(event) {
                match effect {
                    Effect::SubscribeTerminal(pane_id) => {
                        self.send(Command::Subscribe { pane_id });
                    }
                    Effect::Resync => self.engine.resync(),
                }
            }
        }
        cx.notify();
    }

    /// Called while painting a terminal: fits its screen to the room it has and tells the
    /// engine when the size really changed.
    pub fn fit_terminal(&mut self, pane_id: &str, rows: u16, cols: u16) {
        if let Some((rows, cols)) = self.state.fit_terminal(pane_id, rows, cols) {
            self.send(Command::Resize {
                pane_id: pane_id.to_owned(),
                rows,
                cols,
            });
        }
    }

    pub(crate) fn select_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        self.state.select_workspace(workspace_id);
        self.announce_focus();
        cx.notify();
    }

    pub(crate) fn focus_pane(
        &mut self,
        pane_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.focus_pane(pane_id);
        self.announce_focus();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Queues a command for the engine. Typing and clicking never wait for the engine, so a
    /// command the queue cannot take is reported to the user instead.
    fn send(&mut self, command: Command) {
        if let Err(error) = self.engine.send(command) {
            self.state.problem = Some(error.to_string());
        }
    }

    fn announce_focus(&mut self) {
        if let Some(pane_id) = self.state.focused_pane().map(|pane| pane.pane_id.clone()) {
            self.send(Command::Focus { pane_id });
        }
    }

    pub(crate) fn close_pane(&mut self, pane_id: &str) {
        self.send(Command::ClosePane {
            pane_id: pane_id.to_owned(),
        });
    }

    pub(crate) fn new_terminal(&mut self, _: &NewTerminal, _: &mut Window, _: &mut Context<Self>) {
        if let Some(workspace) = self.state.selected_workspace() {
            let workspace_id = workspace.workspace_id.clone();
            self.send(Command::CreateTerminal { workspace_id });
        }
    }

    pub(crate) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let Some(pane_id) = self.focused_terminal() else {
            return;
        };
        let bracketed = self
            .state
            .screen(&pane_id)
            .is_some_and(TerminalScreen::bracketed_paste);
        self.send_input(&pane_id, keys::paste(&text, bracketed), cx);
    }

    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
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
    fn send_input(&mut self, pane_id: &str, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if self.state.connection != Connection::Connected {
            // Keystrokes cannot reach a dead connection; say so instead of dropping them.
            self.state.problem = Some(Error::Disconnected.to_string());
            cx.notify();
            return;
        }
        if let Some(screen) = self.state.screen_mut(pane_id)
            && screen.scrolled_back()
        {
            screen.scroll_to_bottom();
            cx.notify();
        }
        self.send(Command::Input {
            pane_id: pane_id.to_owned(),
            bytes,
        });
    }
}
