//! The harness switch as the Mac's popover does it: the person picks a harness, then a model and
//! an effort, then confirms with "Restart conversation with ...". Nothing is sent until then.
//! The choices live in a [`SwitchDraft`] that lasts as long as the popover is open.

use crate::composer_field::{ComposerEvent, ComposerField, new_field};
use crate::{open_menu::Menu, workbench::Workbench};
use gpui::{Context, Entity};
use soyeht_client::{Command, account::AccountAction, harness_picker, switch::can_switch};
use soyeht_protocol::HarnessInfo;

/// What the open switch popover has chosen so far.
pub struct SwitchDraft {
    pub pane_id: String,
    /// The harness to switch to, once one is chosen.
    pub harness: Option<String>,
    /// The model chosen; `None` leaves the choice to the harness ("Agent default").
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The model list is unfolded under the model field.
    pub models_open: bool,
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
        self.switch = Some(SwitchDraft {
            pane_id: pane_id.to_owned(),
            harness: None,
            model: None,
            effort: None,
            models_open: false,
            search,
        });
        self.send(Command::LoadHarnesses { rescan: false });
        self.choose_first_harness(cx);
    }

    /// The draft of the open popover of `pane_id`, if that is the popover that is open.
    pub(crate) fn switch_draft(&self, pane_id: &str) -> Option<&SwitchDraft> {
        self.switch.as_ref().filter(|draft| {
            draft.pane_id == pane_id && self.menus.is_open(&Menu::Harness(pane_id.to_owned()))
        })
    }

    /// The harness a pane runs.
    fn running_harness(&self, pane_id: &str) -> Option<String> {
        self.state
            .pane(pane_id)
            .and_then(|pane| pane.agent.as_ref())
            .map(|agent| agent.harness.clone())
    }

    /// Chooses the default harness when the one chosen cannot start (or none is), as the Mac
    /// does each time it opens and each time the list changes.
    pub(crate) fn choose_first_harness(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.switch.as_ref() else {
            return;
        };
        let pane_id = draft.pane_id.clone();
        let chosen_can_start = draft.harness.as_deref().is_some_and(|id| {
            self.state
                .harnesses()
                .iter()
                .any(|harness| harness.id == id && harness.available)
        });
        if chosen_can_start {
            return;
        }
        let running = self.running_harness(&pane_id);
        let preferred = self.state.preferences().preferred_harness.clone();
        let first = harness_picker::default_harness(
            self.state.harnesses(),
            running.as_deref(),
            preferred.as_deref(),
        )
        .map(|harness| harness.id.clone());
        if let Some(first) = first {
            self.choose_switch_harness(&first, cx);
        }
    }

    /// The person picked `harness` in the list: the model and effort start over, and the models
    /// of the harness are fetched when it can start.
    pub(crate) fn choose_switch_harness(&mut self, harness: &str, cx: &mut Context<Self>) {
        let available = self
            .state
            .harnesses()
            .iter()
            .any(|known| known.id == harness && known.available);
        let Some(draft) = self.switch.as_mut() else {
            return;
        };
        draft.harness = Some(harness.to_owned());
        draft.model = None;
        draft.effort = None;
        draft.models_open = false;
        if available {
            self.send(Command::LoadCatalogue {
                harness: harness.to_owned(),
            });
        }
        cx.notify();
    }

    pub(crate) fn choose_switch_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if let Some(draft) = self.switch.as_mut() {
            draft.model = model;
            draft.models_open = false;
        }
        cx.notify();
    }

    pub(crate) fn choose_switch_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        if let Some(draft) = self.switch.as_mut() {
            draft.effort = Some(effort);
        }
        cx.notify();
    }

    pub(crate) fn toggle_switch_models(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.switch.as_mut() {
            draft.models_open = !draft.models_open;
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
        let draft = self.switch_draft(pane_id)?;
        let id = draft.harness.as_deref()?;
        self.state
            .harnesses()
            .iter()
            .find(|harness| harness.id == id)
    }

    /// Confirms the switch: the pane restarts on the chosen harness and the conversation is
    /// handed over. The engine's events tell how it went.
    pub(crate) fn confirm_switch(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let allowed = can_switch(
            connected,
            self.state
                .pane(pane_id)
                .and_then(|pane| pane.agent.as_ref()),
            self.state
                .agent(pane_id)
                .and_then(|view| view.harness_switch.as_ref()),
        );
        let Some(target) = self
            .switch_target(pane_id)
            .filter(|target| target.available)
        else {
            return;
        };
        let (harness, model, effort) = match self.switch_draft(pane_id) {
            Some(draft) if allowed => {
                (target.id.clone(), draft.model.clone(), draft.effort.clone())
            }
            _ => return,
        };
        self.send(Command::SwitchHarness {
            pane_id: pane_id.to_owned(),
            harness,
            model,
            effort,
        });
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
