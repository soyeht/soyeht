//! The workbench: the root view. It owns the engine connection and the client state, turns
//! engine events into state changes, and turns keyboard and pointer input into commands.
//! Rendering lives in `views`.

use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    destination::Destination,
    loop_form::LoopForm,
    open_menu::{Menu, OpenMenu},
    tasks_ui::TasksUi,
    theme::ThemeName,
};
use gpui::{
    Context, Entity, FocusHandle, Focusable, KeyDownEvent, ScrollHandle, ScrollWheelEvent, Task,
    Window, actions,
};
use soyeht_client::{
    Command, Connection, Effect, Engine, Error, Event, State,
    attachments::Staged,
    keys::{self, Modifiers},
    markdown,
    screen::TerminalScreen,
};
use soyeht_protocol::{PaneMode, WorkspacePaneLayoutAxis};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

actions!(soyeht, [Quit, NewTerminal, Paste]);

/// Engine events applied per frame at most, so a burst of output cannot starve painting.
const EVENTS_PER_BATCH: usize = 256;

pub struct Workbench {
    engine: Engine,
    pub(crate) state: State,
    pub(crate) theme: ThemeName,
    /// Keyboard focus of the window; terminals take their keys from it.
    pub(crate) focus: FocusHandle,
    pub(crate) destination: Destination,
    pub(crate) rail_open: bool,
    pub(crate) menus: OpenMenu,
    /// The message line of each agent pane's composer.
    pub(crate) fields: HashMap<String, Entity<ComposerField>>,
    /// Numbers the requests that open a pane, so each answer is matched to its request.
    next_ticket: u64,
    transcripts: HashMap<String, (ScrollHandle, usize)>,
    /// The thoughts of an agent the person has opened, by message.
    pub(crate) open_thoughts: HashSet<String>,
    /// The subagent rows the person has opened, by subagent.
    pub(crate) open_subagents: HashSet<String>,
    /// How many messages beyond `TAIL` each agent's transcript draws.
    pub(crate) transcript_extra: HashMap<String, usize>,
    /// When each composer's draft was last announced to the engine.
    pub(crate) draft_beats: HashMap<String, Instant>,
    /// The tasks surface: its view, the task open and the new-task form.
    pub(crate) tasks_ui: TasksUi,
    /// The loop panel each open pane is editing.
    pub(crate) loop_forms: HashMap<String, LoopForm>,
    /// The files attached to each agent pane's next message, with the latest refusal.
    pub(crate) staged: HashMap<String, Staged>,
    /// The sideways scroll of each composer's attachment chips.
    attachment_scrolls: HashMap<String, ScrollHandle>,
    /// The fields of the questions agents ask, by request and field.
    pub(crate) form_fields: HashMap<(String, String), Entity<ComposerField>>,
    /// The Markdown of the replies on screen, parsed once per change.
    pub(crate) markdown: markdown::Cache,
    /// The sideways scroll of the stage, when the panes need more room than the window has.
    pub(crate) stage_scroll: ScrollHandle,
    /// The sideways scroll of the workspace tabs.
    pub(crate) tab_scroll: ScrollHandle,
    /// The workspace whose tab was last scrolled into view.
    pub(crate) revealed_tab: Option<String>,
    /// How many tabs were not wholly in view when the last frame was laid out.
    pub(crate) hidden_tabs: usize,
    /// The sideways scroll of each agent composer's chip strip.
    chip_scrolls: HashMap<String, ScrollHandle>,
    /// The pane to bring into view on the next frame, once the stage has been laid out.
    pub(crate) reveal: Option<String>,
    /// The pane whose input takes keyboard focus on the next frame.
    focus_request: Option<String>,
    _events: Task<()>,
    /// Repaints once a second while an agent runs a turn, so the time in its header counts.
    _clock: Task<()>,
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
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let repaint = this.update(cx, |this, cx| {
                    this.beat_drafts(cx);
                    if this.state.has_running_turn() {
                        cx.notify();
                    }
                });
                if repaint.is_err() {
                    break;
                }
            }
        });
        Self {
            engine,
            state: State::default(),
            theme: ThemeName::DEFAULT,
            focus: cx.focus_handle(),
            destination: Destination::default(),
            rail_open: true,
            menus: OpenMenu::default(),
            fields: HashMap::new(),
            next_ticket: 0,
            transcripts: HashMap::new(),
            open_thoughts: HashSet::new(),
            open_subagents: HashSet::new(),
            transcript_extra: HashMap::new(),
            form_fields: HashMap::new(),
            staged: HashMap::new(),
            loop_forms: HashMap::new(),
            tasks_ui: TasksUi::default(),
            draft_beats: HashMap::new(),
            attachment_scrolls: HashMap::new(),
            markdown: markdown::Cache::default(),
            stage_scroll: ScrollHandle::new(),
            tab_scroll: ScrollHandle::new(),
            revealed_tab: None,
            hidden_tabs: 0,
            chip_scrolls: HashMap::new(),
            reveal: None,
            focus_request: None,
            _events: events,
            _clock: clock,
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
                    // A pane's queue of waiting messages is loaded with its transcript.
                    Effect::LoadTranscript(pane_id) => {
                        self.send(Command::LoadQueue {
                            pane_id: pane_id.clone(),
                        });
                        self.send(Command::LoadTranscript { pane_id });
                    }
                    Effect::LoadUsage => {
                        self.send(Command::LoadUsage);
                    }
                    Effect::LoadHarnesses => {
                        self.send(Command::LoadHarnesses);
                    }
                    Effect::LoadTask {
                        workspace_id,
                        task_id,
                    } => {
                        self.send(Command::LoadTask {
                            workspace_id,
                            task_id,
                        });
                    }
                    Effect::StoreLayout {
                        workspace_id,
                        layout,
                    } => {
                        self.send(Command::SetLayout {
                            workspace_id,
                            layout,
                        });
                    }
                    // The pane the person opened takes the keyboard, as a new terminal does on the Mac.
                    Effect::FocusPane(pane_id) => {
                        // A pane a task was dispatched to is in the agents' place.
                        if !self.destination.is_pane_grid() {
                            self.destination = Destination::Agents;
                        }
                        self.state.focus_pane(&pane_id);
                        self.announce_focus();
                        self.reveal = Some(pane_id.clone());
                        self.focus_request = Some(pane_id);
                    }
                }
            }
        }
        self.forget_closed_panes();
        cx.notify();
    }

    /// Drops what is kept per pane for panes that are gone, and per message for messages that are.
    fn forget_closed_panes(&mut self) {
        let state = &self.state;
        self.fields.retain(|id, _| state.agent(id).is_some());
        self.transcripts.retain(|id, _| state.agent(id).is_some());
        self.chip_scrolls.retain(|id, _| state.agent(id).is_some());
        self.transcript_extra
            .retain(|id, _| state.agent(id).is_some());
        self.staged.retain(|id, _| state.agent(id).is_some());
        self.loop_forms.retain(|id, _| state.agent(id).is_some());
        self.draft_beats.retain(|id, _| state.agent(id).is_some());
        self.attachment_scrolls
            .retain(|id, _| state.agent(id).is_some());
        self.form_fields.retain(|(request, _), _| {
            state.agent_ids().any(|id| {
                state
                    .agent(id)
                    .and_then(|view| view.cards.elicitation.as_ref())
                    .is_some_and(|open| &open.request_id == request)
            })
        });
        if !self.open_thoughts.is_empty() {
            self.open_thoughts.retain(|message| {
                state.agent_ids().any(|id| {
                    state
                        .agent(id)
                        .is_some_and(|view| view.messages.iter().any(|m| &m.local_id == message))
                })
            });
        }
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
        self.menus.close();
        self.focus_request = self.state.focused_pane().map(|pane| pane.pane_id.clone());
        self.reveal.clone_from(&self.focus_request);
        cx.notify();
    }

    pub(crate) fn close_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        self.send(Command::CloseWorkspace {
            workspace_id: workspace_id.to_owned(),
        });
        self.menus.close();
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
        self.menus.close();
        self.reveal = Some(pane_id.to_owned());
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
        let is_agent = self
            .state
            .panes()
            .iter()
            .any(|pane| pane.pane_id == pane_id && pane.mode == PaneMode::Agent);
        if !is_agent {
            return self.focus.clone();
        }
        self.composer_field(pane_id, cx).focus_handle(cx)
    }

    /// The message line of an agent's composer, made when first needed.
    pub(crate) fn composer_field(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> Entity<ComposerField> {
        if let Some(field) = self.fields.get(pane_id) {
            return field.clone();
        }
        let field = new_field(self.theme.palette(), cx);
        let owner = pane_id.to_owned();
        cx.subscribe(
            &field,
            move |this, _, event: &ComposerEvent, cx| match event {
                ComposerEvent::Submit => this.submit_prompt(&owner, cx),
                ComposerEvent::Dismiss => this.close_menu(cx),
            },
        )
        .detach();
        self.fields.insert(pane_id.to_owned(), field.clone());
        field
    }

    /// The sideways scroll of an agent composer's chip strip, made when first needed.
    pub(crate) fn chip_scroll(&mut self, pane_id: &str) -> ScrollHandle {
        self.chip_scrolls
            .entry(pane_id.to_owned())
            .or_default()
            .clone()
    }

    /// The sideways scroll of an agent composer's attachment chips, made when first needed.
    pub(crate) fn attachment_scroll(&mut self, pane_id: &str) -> ScrollHandle {
        self.attachment_scrolls
            .entry(pane_id.to_owned())
            .or_default()
            .clone()
    }

    /// The scroll state of an agent's transcript, following the newest message while it grows.
    pub(crate) fn transcript_scroll(&mut self, pane_id: &str, messages: usize) -> ScrollHandle {
        let (handle, seen) = self
            .transcripts
            .entry(pane_id.to_owned())
            .or_insert_with(|| (ScrollHandle::new(), 0));
        if *seen != messages {
            *seen = messages;
            handle.scroll_to_bottom();
        }
        handle.clone()
    }

    /// Called at the start of a frame: honours a request to move keyboard focus.
    pub(crate) fn apply_focus_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane_id) = self.focus_request.take() {
            self.focus_input(&pane_id, window, cx);
        }
    }

    /// Queues a command for the engine. Typing and clicking never wait for the engine, so a
    /// command the queue cannot take is reported to the user instead; false means it was not taken.
    pub(crate) fn send(&mut self, command: Command) -> bool {
        match self.engine.send(command) {
            Ok(()) => true,
            Err(error) => {
                self.state.problem = Some(error.to_string());
                false
            }
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

    pub(crate) fn new_terminal(&mut self, _: &NewTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.open_pane(None, cx);
    }

    /// Opens a terminal, or an agent running `harness`, in the selected workspace.
    pub(crate) fn open_pane(&mut self, harness: Option<String>, cx: &mut Context<Self>) {
        let Some(workspace) = self.state.selected_workspace() else {
            return;
        };
        let workspace_id = workspace.workspace_id.clone();
        let ticket = self.next_ticket();
        self.state.expect_open(ticket, &workspace_id);
        self.request_pane(workspace_id, harness, ticket, cx);
    }

    /// Splits a pane: the new pane opens beside it, of the same kind.
    pub(crate) fn split_pane(
        &mut self,
        pane_id: &str,
        axis: WorkspacePaneLayoutAxis,
        cx: &mut Context<Self>,
    ) {
        let harness = self
            .state
            .panes()
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.agent.as_ref().map(|agent| agent.harness.clone()));
        let ticket = self.next_ticket();
        if let Some(workspace_id) = self.state.expect_split(ticket, pane_id, axis) {
            self.request_pane(workspace_id, harness, ticket, cx);
        }
    }

    pub(crate) fn next_ticket(&mut self) -> u64 {
        self.next_ticket += 1;
        self.next_ticket
    }

    /// Asks the engine to open the pane that request `ticket` waits for. A request the queue
    /// refuses releases it at once.
    fn request_pane(
        &mut self,
        workspace_id: String,
        harness: Option<String>,
        ticket: u64,
        cx: &mut Context<Self>,
    ) {
        self.menus.close();
        let taken = self.send(match harness {
            Some(harness) => Command::CreateAgent {
                workspace_id,
                harness,
                ticket,
            },
            None => Command::CreateTerminal {
                workspace_id,
                ticket,
            },
        });
        if !taken {
            self.state.apply(Event::OpenFailed { ticket });
        }
        cx.notify();
    }

    pub(crate) fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if self.menus.toggle(menu, Instant::now()) {
            cx.notify();
        }
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menus.close() {
            cx.notify();
        }
    }

    /// A click outside the open menu closed it.
    pub(crate) fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        if self.menus.dismiss(Instant::now()) {
            cx.notify();
        }
    }

    /// Opens or closes the "More harnesses" entry of the open harness picker.
    pub(crate) fn toggle_more_harnesses(&mut self, cx: &mut Context<Self>) {
        self.menus.toggle_more_harnesses();
        cx.notify();
    }

    pub(crate) fn toggle_rail(&mut self, cx: &mut Context<Self>) {
        self.rail_open = !self.rail_open;
        cx.notify();
    }

    pub(crate) fn select_destination(&mut self, destination: Destination, cx: &mut Context<Self>) {
        self.destination = destination;
        if destination == Destination::Tasks {
            self.refresh_tasks();
        }
        cx.notify();
    }

    /// Sends the composer's text to the agent. The text leaves the composer only once the
    /// engine's queue has taken it: a message that could not be sent stays to be sent again.
    pub(crate) fn submit_prompt(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(field) = self.fields.get(pane_id).cloned() else {
            return;
        };
        let Some(text) = field.read(cx).draft().to_send() else {
            return;
        };
        if self.state.connection != Connection::Connected {
            self.state.problem = Some(Error::Disconnected.to_string());
            cx.notify();
            return;
        }
        let attachments = self
            .staged
            .get(pane_id)
            .map(|staged| staged.items().to_vec())
            .unwrap_or_default();
        if self.send(Command::Prompt {
            pane_id: pane_id.to_owned(),
            text,
            attachments,
        }) {
            field.update(cx, |field, cx| field.clear(cx));
            if let Some(staged) = self.staged.get_mut(pane_id) {
                staged.take();
            }
        }
        cx.notify();
    }

    pub(crate) fn toggle_thought(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if !self.open_thoughts.remove(message_id) {
            self.open_thoughts.insert(message_id.to_owned());
        }
        cx.notify();
    }

    pub(crate) fn cancel_turn(&mut self, pane_id: &str) {
        self.send(Command::Cancel {
            pane_id: pane_id.to_owned(),
        });
    }

    pub(crate) fn set_config(
        &mut self,
        pane_id: &str,
        config_id: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        self.send(Command::SetConfig {
            pane_id: pane_id.to_owned(),
            config_id: config_id.to_owned(),
            value: value.to_owned(),
        });
        self.menus.close();
        cx.notify();
    }

    pub(crate) fn switch_harness(&mut self, pane_id: &str, harness: &str, cx: &mut Context<Self>) {
        self.send(Command::SwitchHarness {
            pane_id: pane_id.to_owned(),
            harness: harness.to_owned(),
        });
        self.menus.close();
        cx.notify();
    }

    pub(crate) fn answer_permission(&mut self, pane_id: &str, request_id: &str, option_id: &str) {
        self.send(Command::AnswerPermission {
            pane_id: pane_id.to_owned(),
            request_id: request_id.to_owned(),
            option_id: option_id.to_owned(),
        });
    }

    pub(crate) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(pane) = self.state.focused_pane()
            && pane.mode == PaneMode::Agent
        {
            let pane_id = pane.pane_id.clone();
            // Copied files and pictures become attachments; anything else is text.
            if self.attach_pasted(&pane_id, &item, cx) {
                return;
            }
            if let Some(text) = item.text() {
                let field = self.composer_field(&pane_id, cx);
                field.update(cx, |field, cx| field.insert(&text, cx));
            }
            return;
        }
        let Some(text) = item.text() else {
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
        // Escape closes an open menu, as on the Mac, and is not typed into the terminal.
        if keystroke.key == "escape" && self.menus.escape() {
            cx.notify();
            cx.stop_propagation();
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
