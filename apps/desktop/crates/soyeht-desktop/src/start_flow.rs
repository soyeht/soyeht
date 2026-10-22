//! "Start with an agent": the first-run path that the Mac's new-pane sheet gives in one step. The
//! person picks a folder (the selected workspace's by default), an agent from the ones installed
//! (the default the Mac preselects) and writes the first message; Enter makes the workspace if
//! the folder has none, opens the agent in it and sends the message. The rules (what stops the
//! start, which agent is preselected, the order of the requests) are `soyeht_client::new_agent`
//! and `State::start_agent`; this holds the form's state and carries the person's choices there.

use crate::composer_field::{ComposerEvent, ComposerField, new_field};
use crate::{destination::Destination, workbench::Workbench};
use gpui::{Context, Entity, Focusable, Window};
use soyeht_client::{Command, new_agent::NewAgent};

/// The form while it is open.
pub(crate) struct StartUi {
    pub form: NewAgent,
    /// The message line.
    pub field: Entity<ComposerField>,
    /// The message line has not yet taken the keyboard.
    pub focus: bool,
    /// The form is on the stage; it is kept, with its text, while the person looks elsewhere.
    pub shown: bool,
}

impl Workbench {
    /// Opens the form: the folder of the selected workspace, the agent the Mac would preselect.
    /// A form the person left unsent, by going to another tab or place, comes back as it was.
    pub(crate) fn open_start(&mut self, cx: &mut Context<Self>) {
        self.menus.close();
        if !self.require_connection(cx) {
            return;
        }
        // The Mac checks the agents again whenever its sheet opens.
        self.send(Command::LoadHarnesses { rescan: true });
        if let Some(start) = &mut self.start {
            start.shown = true;
            start.focus = true;
            self.destination = Destination::Agents;
            cx.notify();
            return;
        }
        let form = NewAgent {
            directory: self
                .state
                .selected_workspace()
                .map(|workspace| workspace.path.clone())
                .unwrap_or_default(),
            ..NewAgent::default()
        };
        self.show_start(form, cx);
    }

    /// Shows the form with `form` in it, its message in the message line.
    fn show_start(&mut self, mut form: NewAgent, cx: &mut Context<Self>) {
        form.keep_or_choose_harness(
            self.state.harnesses(),
            self.state.preferences().preferred_harness.as_deref(),
        );
        let field = new_field(self.theme.palette(), cx);
        field.update(cx, |field, cx| field.insert(&form.prompt, cx));
        cx.subscribe(&field, |this, _, event: &ComposerEvent, cx| match event {
            ComposerEvent::Submit => this.submit_start(cx),
            ComposerEvent::Dismiss => {
                if this.menus.close() {
                    cx.notify();
                } else {
                    this.close_start(cx);
                }
            }
            ComposerEvent::Attach => {}
        })
        .detach();
        self.start = Some(StartUi {
            form,
            field,
            focus: true,
            shown: true,
        });
        self.destination = Destination::Agents;
        cx.notify();
    }

    /// The agent that could not start comes back to the form with what the person typed.
    pub(crate) fn restore_failed_start(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.state.take_failed_start() else {
            return;
        };
        if self.start.is_none() {
            self.show_start(form, cx);
        }
        self.state.problem =
            Some("The agent could not start. Your message is back in the form.".to_owned());
        cx.notify();
    }

    /// Takes the form off the stage, keeping what was typed in it.
    pub(crate) fn hide_start(&mut self) {
        if let Some(start) = &mut self.start {
            start.shown = false;
        }
    }

    /// Gives the keyboard to the form's message line, once, after the form opened.
    pub(crate) fn focus_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(start) = &mut self.start
            && std::mem::take(&mut start.focus)
        {
            window.focus(&start.field.focus_handle(cx), cx);
        }
    }

    pub(crate) fn close_start(&mut self, cx: &mut Context<Self>) {
        self.menus.close();
        if self.start.take().is_some() {
            cx.notify();
        }
    }

    /// Shows the platform's folder picker and takes the folder it returns.
    pub(crate) fn choose_start_folder(&mut self, cx: &mut Context<Self>) {
        self.pick_folder(
            "Choose",
            |this, folder, cx| match folder.to_str() {
                Some(path) => this.set_start_folder(path, cx),
                None => {
                    this.state.problem =
                        Some("That folder's name is not text Soyeht can use.".to_owned());
                    cx.notify();
                }
            },
            cx,
        );
    }

    pub(crate) fn set_start_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        self.menus.close();
        if let Some(start) = &mut self.start {
            start.form.directory = path.to_owned();
        }
        cx.notify();
    }

    pub(crate) fn set_start_harness(&mut self, harness: &str, cx: &mut Context<Self>) {
        self.menus.close();
        if let Some(start) = &mut self.start {
            start.form.harness = harness.to_owned();
        }
        cx.notify();
    }

    /// The form as the next frame draws it: the message line's text in it, and an agent kept or
    /// chosen from the ones the engine lists now.
    pub(crate) fn start_form_now(&mut self, cx: &Context<Self>) -> Option<NewAgent> {
        let preferred = self.state.preferences().preferred_harness.clone();
        let harnesses = self.state.harnesses().to_vec();
        let start = self.start.as_mut()?;
        start.form.prompt = start.field.read(cx).draft().text().to_owned();
        start
            .form
            .keep_or_choose_harness(&harnesses, preferred.as_deref());
        Some(start.form.clone())
    }

    /// Starts the agent the form describes. The form closes once the requests are queued.
    pub(crate) fn submit_start(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.start_form_now(cx) else {
            return;
        };
        if !self.require_connection(cx) {
            return;
        }
        let ticket = self.next_ticket();
        match self.state.start_agent(ticket, &form) {
            Ok(started) => {
                self.start = None;
                if let Some(workspace_id) = started.select {
                    self.select_workspace(&workspace_id, cx);
                }
                self.select_destination(Destination::Agents, cx);
                self.run_effects(started.effects);
                self.restore_failed_start(cx);
            }
            Err(reason) => self.state.problem = Some(reason.to_owned()),
        }
        cx.notify();
    }
}
