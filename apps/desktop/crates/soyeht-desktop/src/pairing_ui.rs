//! What the window keeps for the pairing sheet and the list of devices (ADR 0164): the field each
//! request's code is typed in and the "Copied" mark of the link. The rules are
//! `soyeht_client::pairing`; this holds the widgets and carries what the person does to the
//! replica, through [`act`], which also sends what the replica then asks of the engine.

use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    theme::Palette,
    workbench::Workbench,
};
use gpui::{App, ClipboardItem, Context, Entity};
use soyeht_client::{Effect, State, pairing::Rejected, timestamp};
use soyeht_protocol::client_policy::COPIED_FEEDBACK_MS;
use std::{collections::HashMap, time::Duration};

/// How long "Copied" shows after the link is copied.
const COPIED_FOR: Duration = Duration::from_millis(COPIED_FEEDBACK_MS);

#[derive(Default)]
pub(crate) struct PairingUi {
    /// The field of each request that is asked a code, made when the request first shows.
    codes: HashMap<String, Entity<ComposerField>>,
    /// The link was copied a moment ago.
    link_copied: bool,
}

impl PairingUi {
    /// The link was copied a moment ago, and the button says so.
    pub(crate) fn link_copied(&self) -> bool {
        self.link_copied
    }

    /// The field the code of `request_id` is typed in.
    pub(crate) fn code_field(
        &mut self,
        request_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Workbench>,
    ) -> Entity<ComposerField> {
        if let Some(field) = self.codes.get(request_id) {
            return field.clone();
        }
        let field = new_field(palette, cx);
        let id = request_id.to_owned();
        cx.subscribe(&field, move |this, _, event: &ComposerEvent, cx| {
            if *event == ComposerEvent::Submit {
                decide(this, &id, true, cx);
            } else if *event == ComposerEvent::Dismiss {
                close(this, cx);
            }
        })
        .detach();
        self.codes.insert(request_id.to_owned(), field.clone());
        field
    }

    /// Allows or denies `request_id`, with the code in its field. A decision the replica does not
    /// send (not six digits, no code to compare) says why in the request itself.
    fn decide(
        &mut self,
        state: &mut State,
        request_id: &str,
        approve: bool,
        cx: &mut App,
    ) -> Vec<Effect> {
        let typed = self
            .codes
            .get(request_id)
            .map(|field| field.read(cx).draft().text().to_owned())
            .unwrap_or_default();
        match state.decide_pairing(request_id, approve, &typed) {
            Ok(effects) => {
                // The code has gone to the engine: it is not kept in a field.
                if let Some(field) = self.codes.get(request_id) {
                    field.update(cx, |field, cx| field.clear(cx));
                }
                effects
            }
            Err(Rejected::MalformedCode | Rejected::NoComparison | Rejected::Unavailable) => {
                Vec::new()
            }
        }
    }

    /// Puts the link of the window on the clipboard and shows "Copied" for a moment.
    fn copy_link(&mut self, link: String, cx: &mut Context<Workbench>) {
        cx.write_to_clipboard(ClipboardItem::new_string(link));
        self.link_copied = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            // Once the window is gone nobody waits for the mark to go.
            this.update(cx, |this, cx| {
                this.pairing_ui.link_copied = false;
                cx.notify();
            })
        })
        .detach();
    }
}

/// Carries out what the person did: `change` alters the replica and the widgets, and what it
/// returns is what the replica now asks of the engine.
fn act(
    this: &mut Workbench,
    cx: &mut Context<Workbench>,
    change: impl FnOnce(&mut PairingUi, &mut State, &mut Context<Workbench>) -> Vec<Effect>,
) {
    let effects = change(&mut this.pairing_ui, &mut this.state, cx);
    this.run_effects(effects);
    cx.notify();
}

/// The person chose "Pair a phone", or asked for a new window.
pub(crate) fn open(this: &mut Workbench, cx: &mut Context<Workbench>) {
    act(this, cx, |_, state, _| state.open_pairing());
}

/// The person closed the sheet. A request still waiting stays in the list of devices.
pub(crate) fn close(this: &mut Workbench, cx: &mut Context<Workbench>) {
    act(this, cx, |ui, state, _| {
        state.pairing.close();
        ui.link_copied = false;
        Vec::new()
    });
}

pub(crate) fn decide(
    this: &mut Workbench,
    request_id: &str,
    approve: bool,
    cx: &mut Context<Workbench>,
) {
    act(this, cx, |ui, state, cx| {
        ui.decide(state, request_id, approve, cx)
    });
}

/// The person dismisses a request that is over.
pub(crate) fn dismiss(this: &mut Workbench, request_id: &str, cx: &mut Context<Workbench>) {
    act(this, cx, |_, state, _| {
        state.pairing.dismiss(request_id);
        Vec::new()
    });
}

pub(crate) fn ask_to_revoke(this: &mut Workbench, device_id: &str, cx: &mut Context<Workbench>) {
    act(this, cx, |_, state, _| {
        state.pairing.ask_to_revoke(device_id);
        Vec::new()
    });
}

pub(crate) fn cancel_revoke(this: &mut Workbench, cx: &mut Context<Workbench>) {
    act(this, cx, |_, state, _| {
        state.pairing.cancel_revoke();
        Vec::new()
    });
}

pub(crate) fn confirm_revoke(this: &mut Workbench, cx: &mut Context<Workbench>) {
    act(this, cx, |_, state, _| state.confirm_revoke());
}

pub(crate) fn copy_link(this: &mut Workbench, link: String, cx: &mut Context<Workbench>) {
    this.pairing_ui.copy_link(link, cx);
    cx.notify();
}

/// Called each time the window applies events: the window's end is looked at, the fields of
/// requests that are gone are dropped, and the first request that asks for a code takes the
/// keyboard when it arrives, so that the person can type at once.
pub(crate) fn settle(this: &mut Workbench, cx: &mut Context<Workbench>) {
    this.state.pairing.tick(timestamp::now());
    let live = |id: &str| {
        this.state
            .pairing
            .requests()
            .iter()
            .any(|request| request.request_id == id)
    };
    this.pairing_ui.codes.retain(|id, _| live(id));
    if !this.state.pairing.shown() || !this.state.pairing.offers_code() {
        return;
    }
    let waiting = this
        .state
        .pairing
        .requests()
        .iter()
        .find(|request| !request.closed && !this.pairing_ui.codes.contains_key(&request.request_id))
        .map(|request| request.request_id.clone());
    if let Some(id) = waiting {
        let palette = this.theme.palette();
        let field = this.pairing_ui.code_field(&id, palette, cx);
        this.focus_field_when_drawn(&field, cx);
    }
}
