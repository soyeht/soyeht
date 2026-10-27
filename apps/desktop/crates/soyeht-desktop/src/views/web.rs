//! A web pane's body: the controls over the page (back, forward, reload and the address) and the
//! page itself, which the system's web view draws (ADR 0110).

use super::{
    prelude::*,
    ui::{control_band, icon_button, pressable},
};
use crate::{
    composer_field::{ComposerEvent, new_field},
    theme::{Palette, layout, radius, space, type_scale},
    web_surface::new_surface,
};
use soyeht_protocol::WebNavigationAction;
use soyeht_webview::Error;

/// What the pane says when there is no web view to draw it.
fn unavailable_message(error: &Error) -> String {
    match error {
        Error::Unsupported => super::ui::not_available("Web panes"),
        other => format!("Web panes are not available here. {other}."),
    }
}

impl Workbench {
    pub(super) fn web_body(
        &mut self,
        pane_id: &str,
        colors: &'static Palette,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let (palette, cx) = (colors, context);
        let Some(registry) = self.web.clone() else {
            return super::pane::notice_body(palette, &super::ui::not_available("Web panes"));
        };
        if let Some(error) = registry.unavailable() {
            return super::pane::notice_body(palette, &unavailable_message(&error));
        }
        let Some(page) = registry.get(pane_id) else {
            return super::pane::notice_body(palette, "Opening the page\u{2026}");
        };
        let pane_id = pane_id.to_owned();
        let state = page.state();
        let (surface, field) = self.web_widgets(&pane_id, &page, palette, cx);
        let band = Self::web_controls(&pane_id, &page, &state, field, palette, cx);
        // What an agent asks about the signed-in session of this site waits above the page.
        let question = self
            .state
            .web_permission(&pane_id)
            .cloned()
            .map(|request| super::permission::card(&pane_id, &request, palette, cx));
        column()
            .flex_1()
            .min_h_0()
            .child(band)
            .children(question)
            .child(div().flex_1().min_h_0().relative().children(surface).when(
                (state.url.is_empty() || state.url == "about:blank") && !state.loading,
                |area| {
                    area.child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(palette.muted))
                            .text_size(px(type_scale::BODY))
                            .child("Enter an address above."),
                    )
                },
            ))
            .into_any_element()
    }
}

impl Workbench {
    /// The page's surface and the address line of a pane, made when it first needs them and styled
    /// with the palette.
    fn web_widgets(
        &mut self,
        pane_id: &str,
        page: &std::sync::Arc<crate::web_page::WebPage>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> (
        Option<gpui::Entity<crate::web_surface::WebSurface>>,
        Option<gpui::Entity<crate::composer_field::ComposerField>>,
    ) {
        let pane_id = pane_id.to_owned();
        if self.panes.of(&pane_id).web.surface.is_none() {
            let surface = new_surface(page.clone(), palette, cx);
            self.panes.of(&pane_id).web.surface = Some(surface);
        }
        if self.panes.of(&pane_id).web.address.is_none() {
            let field = new_field(palette, cx);
            let owner = pane_id.clone();
            cx.subscribe(&field, move |this, field, event: &ComposerEvent, cx| {
                if *event == ComposerEvent::Submit {
                    let typed = field.read(cx).draft().text().replace(['\n', '\r'], "");
                    // What the person typed is theirs no more once it is sent: the page's own
                    // address replaces it when the page has moved.
                    this.panes.of(&owner).web.shown.clone_from(&typed);
                    this.web_go(&owner, &typed, cx);
                    this.focus_request = Some(owner.clone());
                }
            })
            .detach();
            self.panes.of(&pane_id).web.address = Some(field);
        }
        let ui = self.panes.of(&pane_id);
        let (surface, field) = (ui.web.surface.clone(), ui.web.address.clone());
        if let Some(surface) = &surface {
            surface.update(cx, |surface, _| surface.style(palette));
        }
        if let Some(field) = &field {
            field.update(cx, |field, _| field.style(palette, "Enter an address"));
        }
        (surface, field)
    }

    /// Back, forward and reload, the address line, and the hand-back button while a person has
    /// the pane.
    fn web_controls(
        pane_id: &str,
        page: &std::sync::Arc<crate::web_page::WebPage>,
        state: &soyeht_webview::PageState,
        field: Option<gpui::Entity<crate::composer_field::ComposerField>>,
        colors: &'static Palette,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let (pane_id, palette, cx) = (pane_id.to_owned(), colors, context);
        let steps = [
            (
                Icon::ChevronLeft,
                "Back",
                state.can_go_back,
                WebNavigationAction::Back,
            ),
            (
                Icon::ChevronRight,
                "Forward",
                state.can_go_forward,
                WebNavigationAction::Forward,
            ),
            (
                Icon::Refresh,
                "Reload",
                !state.url.is_empty(),
                WebNavigationAction::Reload,
            ),
        ]
        .map(|(glyph, tip, enabled, action)| {
            let owner = pane_id.clone();
            pressable(
                icon_button(
                    super::ui::id(tip, &pane_id),
                    glyph,
                    palette.secondary,
                    tip,
                    palette,
                )
                .size(px(layout::SEARCH_FIELD_HEIGHT))
                .justify_center(),
                enabled,
                cx,
                move |this, cx| this.web_step(&owner, action.clone(), cx),
            )
        });
        let person_has_it = page.control().person_has_control();
        control_band(palette)
            .children(steps)
            .child(
                row()
                    .id(super::ui::id("web-address", &pane_id))
                    .flex_1()
                    .min_w_0()
                    .h(px(layout::SEARCH_FIELD_HEIGHT))
                    .text_size(px(type_scale::BODY))
                    .bg(rgb(palette.face))
                    .rounded(px(radius::COMPACT))
                    .px(px(space::X8))
                    .own_press()
                    .child(div().flex_1().min_w_0().overflow_hidden().children(field)),
            )
            .children(person_has_it.then(|| {
                let owner = pane_id.clone();
                let page = page.clone();
                pressable(
                    super::cards::button(
                        super::ui::id("web-hand-back", &pane_id),
                        super::cards::Role::Secondary,
                        "Hand back to the agent",
                        palette,
                    ),
                    true,
                    cx,
                    move |this, cx| {
                        page.control().hand_back();
                        this.web_state(&owner, cx);
                    },
                )
            }))
            .into_any_element()
    }
}

impl Workbench {
    /// The web place when the workspace has no web pane: the one thing to do there.
    pub(super) fn web_empty(
        &mut self,
        cx: &mut Context<Self>,
        palette: &'static Palette,
    ) -> AnyElement {
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        super::ui::pane_card_shell(palette)
            .child(
                super::ui::empty_state(
                    palette,
                    Some(Icon::Globe),
                    "Open a page to begin",
                    Some("Your own login stays in this pane.".into()),
                )
                .child(pressable(
                    super::cards::button(
                        super::ui::id("open-web", "place"),
                        super::cards::Role::Primary,
                        "Open a web pane",
                        palette,
                    ),
                    connected,
                    cx,
                    Workbench::open_web,
                )),
            )
            .into_any_element()
    }
}
