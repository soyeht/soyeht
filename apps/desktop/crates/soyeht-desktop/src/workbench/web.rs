//! Web panes: the pages the window shows, kept in step with the panes the engine holds, and what
//! the person asks of them (an address, back, forward, reload, a new pane).

use super::Workbench;
use crate::web_page::{Signal, WebRegistry};
use gpui::{Context, Entity, Focusable};
use soyeht_client::{Command, Event};
use soyeht_protocol::{PaneMode, WebNavigationAction};
use std::sync::Arc;

impl Workbench {
    /// Gives the window its web view: the pages, and the signals they raise.
    pub(crate) fn attach_web(
        &mut self,
        registry: Arc<WebRegistry>,
        signals: async_channel::Receiver<Signal>,
        cx: &mut Context<Self>,
    ) {
        self.web = Some(registry);
        self._web_signals = Some(cx.spawn(async move |this, cx| {
            while let Ok(signal) = signals.recv().await {
                if this
                    .update(cx, |this, cx| this.web_signal(&signal, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Makes the page of each web pane that has none and drops the pages of closed panes, when
    /// the engine's events say a pane may have come or gone.
    pub(super) fn sync_web(&mut self, events: &[Event]) {
        let Some(registry) = &self.web else {
            return;
        };
        if events
            .iter()
            .all(|event| matches!(event, Event::Terminal(_)))
        {
            return;
        }
        registry.sync(
            self.state
                .panes()
                .iter()
                .filter(|pane| pane.mode == PaneMode::Web),
        );
    }

    fn web_signal(&mut self, signal: &Signal, cx: &mut Context<Self>) {
        match signal {
            Signal::Created(_) => cx.notify(),
            Signal::Frame(pane_id) => {
                if let Some(surface) = self
                    .panes
                    .get(pane_id)
                    .and_then(|ui| ui.web.surface.clone())
                {
                    surface.update(cx, |_, cx| cx.notify());
                }
            }
            Signal::State(pane_id) => self.web_state(pane_id, cx),
        }
    }

    /// The page moved, or finished loading: the address shows it, and the engine is told where the
    /// page is once it has loaded.
    pub(crate) fn web_state(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(page) = self.web.as_ref().and_then(|web| web.get(pane_id)) else {
            return;
        };
        let url = page.state().url;
        if let Some(ui) = self.panes.get_mut(pane_id)
            && let Some(field) = ui.web.address.clone()
            && ui.web.shown != url
        {
            // An address the person has begun to edit is left as it is.
            let edited = field.read(cx).draft().text() != ui.web.shown;
            if !edited {
                field.update(cx, |field, cx| field.replace(&url, cx));
                ui.web.shown = url;
            }
        }
        if let Some((generation, url, title)) = page.unreported() {
            self.send(Command::ReportWebPage {
                pane_id: pane_id.to_owned(),
                generation,
                url,
                title,
            });
        }
        cx.notify();
    }

    /// Sends a web pane to `address`. The engine checks it and asks this window's page to go.
    pub(crate) fn web_go(&mut self, pane_id: &str, address: &str, cx: &mut Context<Self>) {
        let address = address.trim();
        if address.is_empty() {
            return;
        }
        self.send(Command::NavigateWeb {
            pane_id: pane_id.to_owned(),
            url: Some(address.to_owned()),
            action: None,
        });
        cx.notify();
    }

    /// Back, forward or again.
    pub(crate) fn web_step(
        &mut self,
        pane_id: &str,
        action: WebNavigationAction,
        cx: &mut Context<Self>,
    ) {
        self.send(Command::NavigateWeb {
            pane_id: pane_id.to_owned(),
            url: None,
            action: Some(action),
        });
        cx.notify();
    }

    /// Opens a web pane, empty, in the selected workspace.
    pub(crate) fn open_web(&mut self, cx: &mut Context<Self>) {
        let Some(workspace_id) = self
            .state
            .selected_workspace()
            .map(|w| w.workspace_id.clone())
        else {
            return;
        };
        let ticket = self.next_ticket();
        self.state.expect_open(ticket, &workspace_id);
        let open = Command::OpenWeb {
            workspace_id,
            url: None,
            incognito: false,
            ticket,
        };
        self.send_open(open, ticket, cx);
    }

    /// Where a web pane takes its keys: the address while the page is empty, else the page.
    pub(crate) fn web_focus(
        &mut self,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::FocusHandle> {
        let ui = self.panes.get(pane_id)?;
        let field: Option<Entity<crate::composer_field::ComposerField>> = ui.web.address.clone();
        let surface = ui.web.surface.clone();
        let typed = field
            .as_ref()
            .is_some_and(|field| !field.read(cx).draft().text().is_empty());
        let empty = !typed
            && self
                .web
                .as_ref()
                .and_then(|web| web.get(pane_id))
                .is_none_or(|page| page.state().url.is_empty());
        if empty && let Some(field) = field {
            return Some(field.focus_handle(cx));
        }
        surface.map(|surface| surface.focus_handle(cx))
    }
}
