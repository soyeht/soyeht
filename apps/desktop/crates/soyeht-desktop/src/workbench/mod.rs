//! The workbench: the root view. It owns the engine connection and the client state, turns
//! engine events into state changes, and turns keyboard and pointer input into commands.
//! Rendering lives in `views`.

use crate::{
    alerts::Alerts,
    destination::Destination,
    open_menu::OpenMenu,
    pane_ui::{PaneUis, ProjectPanels},
    prefs::Prefs,
    rename::Rename,
    stage::Stage,
    start_flow::StartUi,
    switch_flow::SwitchDraft,
    tasks_ui::TasksUi,
    theme::ThemeName,
    views::Replies,
    window_size::WindowSize,
    writing_ui::Places,
};
use gpui::{Context, FocusHandle, Task, actions};
use soyeht_client::{
    Command, Engine, Event, State, loops, screen::TerminalScreen, state::problem::kind,
};
use soyeht_platform::log::Failure;

mod effects;
mod focus;
mod input;
mod lifecycle;
mod menus;

actions!(soyeht, [Quit, NewTerminal, Paste, CopySelection]);

/// Engine events applied per frame at most, so a burst of output cannot starve painting.
const EVENTS_PER_BATCH: usize = 256;

pub struct Workbench {
    engine: Engine,
    pub(crate) state: State,
    pub(crate) theme: ThemeName,
    /// Where the choices this device keeps for itself are written; `None` writes nowhere.
    pub(crate) preferences: Option<std::path::PathBuf>,
    /// Which standing note `state.notice` holds, so that dismissing it is remembered.
    pub(crate) notice_id: Option<&'static str>,
    /// Keyboard focus of the window; terminals take their keys from it.
    pub(crate) focus: FocusHandle,
    pub(crate) destination: Destination,
    pub(crate) rail_open: bool,
    pub(crate) menus: OpenMenu,
    /// What the open harness switch popover has chosen so far.
    pub(crate) switch: Option<SwitchDraft>,
    /// The size of the window as the last frame saw it, for popovers that fit inside it.
    pub(crate) window: WindowSize,
    /// Which Preferences panel is open and what is half done in it.
    pub(crate) prefs: Prefs,
    /// What the window keeps for each agent pane, dropped as one when the pane closes.
    pub(crate) panes: PaneUis,
    /// Numbers the requests that open a pane, so each answer is matched to its request.
    next_ticket: u64,
    /// The tasks surface: its view, the task open and the new-task form.
    pub(crate) tasks_ui: TasksUi,
    /// The Markdown of the replies on screen, parsed once per change.
    pub(crate) markdown: Replies,
    /// The "Start with an agent" form while it is open.
    pub(crate) start: Option<StartUi>,
    /// How the panes lie on the stage: scrolls, the pane to reveal and the drags.
    pub(crate) stage: Stage,
    /// The pane whose input takes keyboard focus on the next frame.
    pub(crate) focus_request: Option<String>,
    /// The name of a workspace or pane being edited in place.
    pub(crate) rename: Option<Rename>,
    /// What each workspace's editor place keeps of its project panel.
    pub(crate) panels: ProjectPanels,
    /// The system notifications for requests agents wait on.
    pub(crate) alerts: Alerts,
    /// The places that list documents: each one's search and new-document form.
    pub(crate) places: Places,
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
                        // Nothing more is waiting, or the channel closed, which the next
                        // `recv` above reports by ending the loop.
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
                cx.background_executor().timer(loops::CLOCK_BEAT).await;
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
            state: State::default().with_sizing(crate::views::sizing()),
            theme: ThemeName::DEFAULT,
            preferences: None,
            notice_id: None,
            focus: cx.focus_handle(),
            destination: Destination::default(),
            rail_open: true,
            menus: OpenMenu::default(),
            switch: None,
            window: WindowSize::default(),
            prefs: Prefs::default(),
            panes: PaneUis::default(),
            next_ticket: 0,
            tasks_ui: TasksUi::default(),
            markdown: Replies::default(),
            start: None,
            stage: Stage::new(),
            focus_request: None,
            rename: None,
            panels: ProjectPanels::default(),
            alerts: Alerts::new(),
            places: Places::default(),
            _events: events,
            _clock: clock,
        }
    }

    pub fn screen(&self, pane_id: &str) -> Option<&TerminalScreen> {
        self.state.screen(pane_id)
    }

    fn apply(&mut self, events: Vec<Event>, cx: &mut Context<Self>) {
        // A frame costs the same however little changed in it, and on a machine without a GPU that
        // is long: output of terminals the window does not show asks for none.
        let mut repaint = false;
        for event in events {
            repaint |= self.state.changes_view(&event);
            let effects = self.state.apply(event);
            self.run_effects(effects);
        }
        self.forget_closed_panes();
        self.raise_alerts(cx);
        self.restore_failed_start(cx);
        if self.switch.is_some() {
            self.choose_first_harness(cx);
        }
        if repaint {
            cx.notify();
        }
    }

    /// Queues a command for the engine. Typing and clicking never wait for the engine, so a
    /// command the queue cannot take is reported to the user instead; false means it was not taken.
    pub(crate) fn send(&mut self, command: Command) -> bool {
        match self.engine.send(command) {
            Ok(()) => true,
            Err(error) => {
                self.state.report_failure(
                    kind::REQUEST_FAILED,
                    &error.failure_kind(),
                    error.to_string(),
                );
                false
            }
        }
    }
}
