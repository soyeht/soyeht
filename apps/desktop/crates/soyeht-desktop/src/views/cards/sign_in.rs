//! The sign-in card: the choices when the agent needs one, the progress of one that runs, and
//! why one did not finish.

use super::{Role, button, caption, card, heading};
use crate::{
    theme::{Palette, desktop, pane_interior, space},
    views::prelude::*,
};
use gpui::{Animation, AnimationExt, relative};
use soyeht_client::cards::{self, SignInState, sign_in as sign_in_words};
use std::time::Duration;

impl Workbench {
    /// The choices when the agent needs a sign-in, the progress of one that runs, and why one
    /// did not finish.
    pub(super) fn sign_in_card(
        &mut self,
        pane_id: &str,
        state: &SignInState,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let title = state.title()?;
        let (glyph, color) = match state {
            SignInState::Failed { .. } => (Icon::Alert, palette.warning()),
            SignInState::Waiting { .. } => (Icon::Compass, palette.ink),
            SignInState::Verifying | SignInState::SigningOut => (Icon::Clock, palette.ink),
            SignInState::Hidden | SignInState::Choose { .. } => (Icon::Key, palette.warning()),
        };
        let content = match state {
            SignInState::Hidden => None,
            SignInState::Choose { methods, failure } => {
                Some(self.sign_in_choices(pane_id, methods, failure.as_deref(), palette, cx))
            }
            SignInState::Waiting { terminal_pane_id } => {
                Some(self.sign_in_waiting(pane_id, terminal_pane_id.as_deref(), palette, cx))
            }
            SignInState::Verifying | SignInState::SigningOut => Some(progress(palette)),
            SignInState::Failed { .. } => {
                let pane_id = pane_id.to_owned();
                Some(
                    row()
                        .justify_end()
                        .child(
                            button(
                                id("sign-in-dismiss", &pane_id),
                                Role::Secondary,
                                sign_in_words::DISMISS,
                                palette,
                            )
                            .on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.dismiss_sign_in(&pane_id, cx)
                                },
                            )),
                        )
                        .into_any_element(),
                )
            }
        };
        Some(
            card(pane_interior::ROW_SPACING, palette)
                .child(heading(glyph, color, title))
                .children(
                    state
                        .detail()
                        .map(|detail| caption(palette.secondary, detail.to_owned())),
                )
                .children(content)
                .into_any_element(),
        )
    }

    fn sign_in_choices(
        &mut self,
        pane_id: &str,
        methods: &[cards::SignInMethod],
        failure: Option<&str>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opens_terminal = methods.iter().any(|method| method.opens_terminal);
        column()
            .gap(px(pane_interior::ROW_SPACING))
            .children(failure.map(|text| caption(palette.warning(), text.to_owned())))
            .children(
                opens_terminal
                    .then(|| caption(palette.secondary, sign_in_words::CHOOSE_TERMINAL_HINT)),
            )
            .children(methods.iter().enumerate().map(|(index, method)| {
                let (pane_id, method_id) = (pane_id.to_owned(), method.id.clone());
                let role = if index == 0 {
                    Role::Primary
                } else {
                    Role::Secondary
                };
                let ink = if role == Role::Primary {
                    palette.face
                } else {
                    palette.ink
                };
                let label = row()
                    .justify_center()
                    .gap(px(space::X6))
                    .child(icon(
                        if method.opens_terminal {
                            Icon::Terminal
                        } else {
                            Icon::ArrowRightCircle
                        },
                        desktop::INLINE_GLYPH,
                        ink,
                    ))
                    .child(SharedString::from(method.name.clone()));
                column()
                    .gap(px(pane_interior::TIGHT_SPACING))
                    .child(
                        button(id("sign-in-method", &method.id), role, "", palette)
                            .w_full()
                            .child(label)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.start_sign_in(&pane_id, Some(method_id.clone()), cx)
                            })),
                    )
                    .children(
                        method
                            .detail
                            .clone()
                            .map(|detail| caption(palette.secondary, detail)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn sign_in_waiting(
        &mut self,
        pane_id: &str,
        terminal: Option<&str>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (show, cancel) = (pane_id.to_owned(), pane_id.to_owned());
        let terminal = terminal.map(str::to_owned);
        row()
            .gap(px(pane_interior::STACK_SPACING))
            .child(progress(palette))
            .children(terminal.map(|terminal| {
                button(
                    id("sign-in-terminal", &show),
                    Role::Secondary,
                    sign_in_words::SHOW_TERMINAL,
                    palette,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.focus_pane(&terminal, window, cx)
                }))
            }))
            .child(
                button(
                    id("sign-in-cancel", &cancel),
                    Role::Destructive,
                    sign_in_words::CANCEL_SIGN_IN,
                    palette,
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, _| this.cancel_sign_in(&cancel)),
                ),
            )
            .into_any_element()
    }
}

/// An indeterminate bar: a segment of ink runs along a well, in palette roles.
fn progress(palette: &Palette) -> AnyElement {
    div()
        .relative()
        .flex_1()
        .h(px(pane_interior::TIGHT_SPACING))
        .overflow_hidden()
        .rounded_full()
        .bg(rgb(palette.well))
        .child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(desktop::PROGRESS_SEGMENT))
                .rounded_full()
                .bg(rgb(palette.ink))
                .with_animation(
                    "sign-in-progress",
                    Animation::new(Duration::from_secs_f32(desktop::PROGRESS_PERIOD)).repeat(),
                    |segment, delta| {
                        segment.left(relative(
                            (1.0 + desktop::PROGRESS_SEGMENT) * delta - desktop::PROGRESS_SEGMENT,
                        ))
                    },
                ),
        )
        .into_any_element()
}
