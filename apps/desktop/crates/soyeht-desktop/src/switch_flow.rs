//! The harness switch as the Mac's popover does it: the person picks a harness, then a model and
//! an effort, then confirms with "Restart conversation with ...". Nothing is sent until then.
//! The choices are `soyeht_client::switch::Draft`, which lasts as long as the popover is open;
//! this holds it with the popover's search field and sends what it asks for.

use crate::composer_field::{ComposerEvent, ComposerField, new_field};
use crate::{open_menu::Menu, workbench::Workbench};
use gpui::{Context, Entity};
use soyeht_client::{Command, account::AccountAction, switch::Draft};
use soyeht_protocol::HarnessInfo;

/// What the open switch popover has chosen so far, and its search field.
pub struct SwitchDraft {
    pub choices: Draft,
    /// The search field above the list of harnesses.
    pub search: Entity<ComposerField>,
}

impl Workbench {
    /// The popover of `pane_id` has just opened: it starts on the default harness, and asks the
    /// engine for fresh readiness and for the models of that harness.
    pub(crate) fn begin_switch(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let search = new_field(self.theme.palette(), cx);
        cx.subscribe(&search, |this, _, event: &ComposerEvent, cx| {
            if *event == ComposerEvent::Dismiss {
                this.close_menu(cx);
            }
        })
        .detach();
        let (choices, requests) = Draft::open(&self.state, pane_id);
        self.switch = Some(SwitchDraft { choices, search });
        for request in requests {
            self.send(request);
        }
        if self
            .switch
            .as_ref()
            .is_some_and(|draft| draft.choices.harness.is_some())
        {
            cx.notify();
        }
    }

    /// The draft of the open popover of `pane_id`, if that is the popover that is open.
    pub(crate) fn switch_draft(&self, pane_id: &str) -> Option<&SwitchDraft> {
        self.switch.as_ref().filter(|draft| {
            draft.choices.pane_id == pane_id
                && self.menus.is_open(&Menu::Harness(pane_id.to_owned()))
        })
    }

    /// Chooses the default harness when the one chosen cannot start (or none is), as the Mac
    /// does each time it opens and each time the list changes.
    pub(crate) fn choose_first_harness(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.switch.as_mut() else {
            return;
        };
        if let Some(request) = draft.choices.settle(&self.state) {
            self.send(request);
            cx.notify();
        }
    }

    /// The person picked `harness` in the list: the model and effort start over, and the models
    /// of the harness are fetched when it can start.
    pub(crate) fn choose_switch_harness(&mut self, harness: &str, cx: &mut Context<Self>) {
        let Some(draft) = self.switch.as_mut() else {
            return;
        };
        if let Some(request) = draft.choices.choose_harness(&self.state, harness) {
            self.send(request);
        }
        cx.notify();
    }

    pub(crate) fn choose_switch_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if let Some(draft) = self.switch.as_mut() {
            draft.choices.choose_model(model);
        }
        cx.notify();
    }

    pub(crate) fn choose_switch_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        if let Some(draft) = self.switch.as_mut() {
            draft.choices.choose_effort(effort);
        }
        cx.notify();
    }

    pub(crate) fn toggle_switch_models(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.switch.as_mut() {
            draft.choices.toggle_models();
        }
        cx.notify();
    }

    /// Scans the machine for installed agents again.
    pub(crate) fn rescan_harnesses(&mut self, cx: &mut Context<Self>) {
        self.send(Command::LoadHarnesses { rescan: true });
        cx.notify();
    }

    /// Whether the harness chosen in the popover can be switched to now.
    pub(crate) fn switch_target(&self, pane_id: &str) -> Option<&HarnessInfo> {
        self.switch_draft(pane_id)?.choices.target(&self.state)
    }

    /// Confirms the switch: the pane restarts on the chosen harness and the conversation is
    /// handed over. The engine's events tell how it went.
    pub(crate) fn confirm_switch(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(request) = self
            .switch_draft(pane_id)
            .and_then(|draft| draft.choices.confirm(&self.state))
        else {
            return;
        };
        self.send(request);
        self.menus.close();
        cx.notify();
    }
}

impl Workbench {
    /// Asks for the models of `harness` again after they could not be read.
    pub(crate) fn retry_catalogue(&mut self, harness: &str, cx: &mut Context<Self>) {
        self.send(Command::LoadCatalogue {
            harness: harness.to_owned(),
        });
        cx.notify();
    }

    /// Runs one thing the account section offers; the engine runs the sign-in in a terminal pane
    /// next to the agent and restarts the session afterwards, so the conversation stays.
    pub(crate) fn run_account_action(
        &mut self,
        pane_id: &str,
        action: &AccountAction,
        cx: &mut Context<Self>,
    ) {
        let pane_id = pane_id.to_owned();
        self.send(match action {
            AccountAction::SwitchTo(method) => Command::StartSignIn {
                pane_id,
                method_id: Some(method.clone()),
            },
            AccountAction::SignOut => Command::SignOut { pane_id },
        });
        self.menus.close();
        cx.notify();
    }
}
