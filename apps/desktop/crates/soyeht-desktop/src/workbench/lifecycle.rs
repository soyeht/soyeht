//! Opening and closing workspaces and panes.

use super::{NewTerminal, Workbench};
use crate::destination::Destination;
use gpui::{Context, Window};
use soyeht_client::{Command, Event};

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

    pub(crate) fn close_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        self.send(Command::CloseWorkspace {
            workspace_id: workspace_id.to_owned(),
        });
        self.menus.close();
        cx.notify();
    }

    /// Closes a pane. An editor pane is a tab, and goes only after its edits have reached the
    /// engine, which keeps the draft.
    pub(crate) fn close_pane(&mut self, pane_id: &str) {
        let effects = self.state.close_pane_effects(pane_id);
        self.run_effects(effects);
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
        self.menus.close();
        // A pane starts in the folder of its workspace, as the Mac asks.
        let cwd = self
            .state
            .workspace(&workspace_id)
            .map(|workspace| workspace.path.clone());
        let taken = self.send(match harness {
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
        });
        if !taken {
            self.state.apply(Event::OpenFailed { ticket });
        }
        cx.notify();
    }
}
