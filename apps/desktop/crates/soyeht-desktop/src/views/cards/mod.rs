//! What an agent pane draws between its transcript and its composer, and the rows that hold a
//! subagent's activity: the silence notice, the sign-in card, the agent's question, the queue of
//! messages waiting for it and the subagent rows. All flat cards of the approved interior; the
//! wording and the rules are the Mac's and come from `soyeht_client::cards`.

mod elicitation;
mod queue;
mod sign_in;
mod transcript;

use super::{
    agent::{labelled, pill},
    prelude::*,
};
use crate::theme::{Palette, pane_interior};
use soyeht_client::{
    cards::{SignInState, SilencePresentation, silence as silence_words},
    timestamp,
};
use soyeht_protocol::Pane;
pub(in crate::views) use transcript::Windowing;

/// How the buttons of the cards are filled: the primary action in ink, the others on a well, a
/// rejection in the danger color.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Primary,
    Secondary,
    Destructive,
}

pub(super) fn button(
    key: SharedString,
    role: Role,
    text: impl Into<SharedString>,
    palette: &Palette,
) -> gpui::Stateful<Div> {
    let (fill, ink) = match role {
        Role::Primary => (palette.ink, palette.face),
        Role::Secondary => (palette.well, palette.ink),
        Role::Destructive => (palette.well, palette.danger()),
    };
    pill(key, fill, ink).child(text.into())
}

/// The frame of a card: a canvas card with its content spaced as the Mac's.
fn card(spacing: f32, palette: &Palette) -> Div {
    column()
        .gap(px(spacing))
        .p(px(pane_interior::CARD_PADDING))
        .rounded(px(pane_interior::CARD_RADIUS))
        .bg(rgb(palette.canvas))
}

/// A card's place under the transcript: the pane's margin at the sides and a gap below.
pub(super) fn lane(card: AnyElement) -> Div {
    div()
        .px(px(pane_interior::CARD_PADDING))
        .pb(px(pane_interior::ROW_SPACING))
        .child(card)
}

/// A card's title: a small icon and a heavy line of text.
fn heading(glyph: Icon, color: u32, text: impl Into<SharedString>) -> Div {
    labelled(glyph, color, text)
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_size(px(pane_interior::HEADING_POINT_SIZE))
}

pub(super) fn caption(color: u32, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(color))
        .child(text.into())
}

impl Workbench {
    /// "No output from the agent for 2m 34s", with the way out.
    pub(super) fn silence_card(
        &mut self,
        pane: &Pane,
        silent_since: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let silence = SilencePresentation::new(silent_since, Some(&pane.state), timestamp::now());
        let (glyph, color) = if silence.escalated {
            (Icon::ClockAlert, palette.warning())
        } else {
            (Icon::Clock, palette.secondary)
        };
        let pane_id = pane.pane_id.clone();
        card(pane_interior::COMPACT_SPACING, palette)
            .child(heading(glyph, color, silence.title()))
            .child(caption(palette.secondary, silence.detail()))
            .child(
                row().justify_end().child(
                    button(
                        id("silence-cancel", &pane_id),
                        Role::Destructive,
                        silence_words::CANCEL_TURN,
                        palette,
                    )
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, _| this.cancel_turn(&pane_id)),
                    ),
                ),
            )
            .into_any_element()
    }

    /// The cards between the transcript and the composer, in the Mac's order: the silence notice,
    /// the sign-in card, the permission request, the agent's question and the queue.
    pub(super) fn cards_under_transcript(
        &mut self,
        pane: &Pane,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let pane_id = pane.pane_id.as_str();
        // Only what the cards need is copied, never the transcript.
        let Some((state, permission)) = self
            .state
            .agent(pane_id)
            .map(|view| (view.cards.clone(), view.permission.clone()))
        else {
            return Vec::new();
        };
        let mut cards: Vec<AnyElement> = Vec::new();
        if let Some(silent_since) = pane.agent.as_ref().and_then(|a| a.silent_since.as_deref()) {
            cards.push(lane(self.silence_card(pane, silent_since, palette, cx)).into_any_element());
        }
        let sign_in = SignInState::new(
            pane.agent.as_ref(),
            &state.auth_methods,
            state.sign_in_failure.as_deref(),
        );
        cards.extend(
            self.sign_in_card(pane_id, &sign_in, palette, cx)
                .map(|card| lane(card).into_any_element()),
        );
        if let Some(request) = &permission {
            cards.push(
                lane(super::permission::card(pane_id, request, palette, cx)).into_any_element(),
            );
        }
        if let Some(request) = &state.elicitation {
            cards.push(lane(self.elicitation_card(request, palette, cx)).into_any_element());
        }
        cards.extend(
            self.queue_card(pane_id, &state, palette, cx)
                .map(|card| lane(card).into_any_element()),
        );
        cards
    }
}
