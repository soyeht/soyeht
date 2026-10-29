//! Opening and closing workspaces and panes.

use super::{NewTerminal, Workbench};
use crate::destination::Destination;
use gpui::{Context, Window};
use soyeht_client::{
    Command, Event,
    pane_close::{self, Closing},
};

impl Workbench {
    pub(crate) fn select_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        self.hide_start();
        self.state.select_workspace(workspace_id);
        self.announce_focus();
        self.menus.close();
        self.focus_request = self.state.focused_pane().map(|pane| pane.pane_id.clone());
        self.stage.reveal.clone_from(&self.focus_request);
        cx.notify();
    }

    /// The close button of a workspace tab: it asks first when the workspace holds a pane, since
    /// they close with it, as the Mac and the iPhone do (`pane_close::workspace_confirmation`).
    pub(crate) fn request_close_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        self.menus.close();
        match self.workspace_question(workspace_id) {
            Some(Closing::Confirm { .. }) => self.closing_workspace = Some(workspace_id.to_owned()),
            _ => self.close_workspace(workspace_id, cx),
        }
        cx.notify();
    }

    /// The answer to the question closing a workspace asked.
    pub(crate) fn answer_workspace_close(&mut self, close: bool, cx: &mut Context<Self>) {
        if let Some(workspace_id) = self.closing_workspace.take()
            && close
        {
            self.close_workspace(&workspace_id, cx);
        }
        cx.notify();
    }

    /// What closing the workspace asks now, from the panes it holds; `None` for a workspace the
    /// engine no longer lists.
    pub(crate) fn workspace_question(&self, workspace_id: &str) -> Option<Closing> {
        let workspace = self.state.workspace(workspace_id)?;
        Some(pane_close::workspace_confirmation(
            &workspace.name,
            self.state.panes_of(workspace_id).count(),
        ))
    }

    /// Closes a workspace after what was typed in its documents has reached the engine.
    fn close_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let effects = self.state.close_workspace_effects(workspace_id);
        self.run_effects(effects);
        cx.notify();
    }

    /// Closes a pane. An editor pane is a tab, and goes only after its edits have reached the
    /// engine, which keeps the draft.
    pub(crate) fn close_pane(&mut self, pane_id: &str) {
        let effects = self.state.close_pane_effects(pane_id);
        self.run_effects(effects);
    }

    /// The close button of a pane: a text or a screenplay asks when it has edits not saved, a
    /// pane that runs a process (a terminal, an agent) always asks, as the Mac does, and the
    /// rest close at once.
    pub(crate) fn request_close_pane(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if self.is_document(pane_id) {
            self.request_close_writing(pane_id, cx);
            return;
        }
        let Some(mode) = self.state.pane(pane_id).map(|pane| pane.mode.clone()) else {
            return;
        };
        self.ask_before_closing(pane_id, pane_close::confirmation(&mode), cx);
    }

    /// Closes the pane at once, or shows the question that has to be answered first.
    pub(crate) fn ask_before_closing(
        &mut self,
        pane_id: &str,
        closing: Closing,
        cx: &mut Context<Self>,
    ) {
        match closing {
            Closing::Immediately => self.close_pane(pane_id),
            asked => self.panes.of(pane_id).closing = Some(asked),
        }
        cx.notify();
    }

    /// The answer to the question closing asked.
    pub(crate) fn answer_close(&mut self, pane_id: &str, close: bool, cx: &mut Context<Self>) {
        self.panes.of(pane_id).closing = None;
        if close {
            self.close_pane(pane_id);
        }
        cx.notify();
    }

    /// The "Editor" entry of the new-pane menu: the editor place of the selected workspace, as
    /// the rail's Editor opens it. The tab that shows takes the keyboard; a workspace with no
    /// file open shows the project panel to pick one from.
    pub(crate) fn open_editor_place(&mut self, cx: &mut Context<Self>) {
        self.menus.close();
        if let Some(workspace_id) = self.state.selected_workspace_id() {
            let effects = self.state.editor_place(&workspace_id);
            self.select_destination(Destination::Editor, cx);
            self.run_effects(effects);
        }
        cx.notify();
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

    pub(crate) fn next_ticket(&mut self) -> u64 {
        self.next_ticket += 1;
        self.next_ticket
    }

    /// Asks the engine to open the pane that request `ticket` waits for. A request the queue
    /// refuses releases it at once.
    pub(crate) fn request_pane(
        &mut self,
        workspace_id: String,
        harness: Option<String>,
        ticket: u64,
        cx: &mut Context<Self>,
    ) {
        // A pane starts in the folder of its workspace, as the Mac asks.
        let cwd = self
            .state
            .workspace(&workspace_id)
            .map(|workspace| workspace.path.clone());
        self.send_open(
            match harness {
                Some(harness) => Command::CreateAgent {
                    workspace_id,
                    harness,
                    cwd,
                    ticket,
                },
                None => Command::CreateTerminal {
                    workspace_id,
                    cwd,
                    ticket,
                },
            },
            ticket,
            cx,
        );
    }

    /// Sends the command that opens the pane `ticket` waits for. A queue that refuses it releases
    /// the ticket at once.
    pub(crate) fn send_open(&mut self, command: Command, ticket: u64, cx: &mut Context<Self>) {
        self.menus.close();
        if !self.send(command) {
            self.state.apply(Event::OpenFailed { ticket });
        }
        cx.notify();
    }
}
