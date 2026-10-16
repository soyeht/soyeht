//! What an agent pane draws between its transcript and its composer, and the rows that hold a
//! subagent's activity: the silence notice, the sign-in card, the agent's question, the queue of
//! messages waiting for it and the subagent rows. All flat cards of the approved interior; the
//! wording and the rules are the Mac's and come from `soyeht_client::cards`.

use super::{
    agent::{labelled, pill, transcript_row},
    prelude::*,
    ui::disclosure_chevron,
};
use crate::theme::{Palette, desktop, pane_interior, space};
use gpui::{Animation, AnimationExt, Transformation, percentage, relative};
use soyeht_client::{
    cards::{
        self, CardState, Entry, Group, QueuePresentation, SignInState, SilencePresentation, Status,
        SubagentPresentation, elicitation, queue as queue_words, sign_in as sign_in_words,
        silence as silence_words,
    },
    markdown, timestamp,
    transcript_rows::{self, Child, Rows, Viewport},
};
use soyeht_protocol::{ElicitationRequest, Pane};
use std::{cell::RefCell, time::Duration};

/// What a frame knows about the rows of the transcript it is building.
pub(super) struct Windowing<'a> {
    /// The pane the transcript belongs to.
    pub pane_id: &'a str,
    /// What the rows were laid out at.
    pub rows: &'a RefCell<Rows>,
    /// Where the transcript is scrolled to.
    pub viewport: Viewport,
    /// How many children of the transcript column come before its rows.
    pub leading: usize,
}

/// Stands in for a row that is far from the viewport: as high as the row was when it was built.
fn spacer_row(height: f32) -> AnyElement {
    div().flex_none().w_full().h(px(height)).into_any_element()
}

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

fn caption(color: u32, text: impl Into<SharedString>) -> Div {
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

    /// The question the agent waits on: a field for each property of its schema.
    pub(super) fn elicitation_card(
        &mut self,
        request: &ElicitationRequest,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fields = elicitation::fields(&request.schema);
        let rows: Vec<AnyElement> = fields
            .iter()
            .map(|field| {
                let input = self.form_field(
                    &request.pane_id,
                    &request.request_id,
                    &field.key,
                    &field.title,
                    cx,
                );
                div()
                    .px(px(pane_interior::ROW_SPACING))
                    .py(px(pane_interior::ROW_VERTICAL_PADDING))
                    .rounded(px(pane_interior::WELL_RADIUS))
                    .bg(rgb(palette.well))
                    .child(input)
                    .into_any_element()
            })
            .collect();
        let (pane_id, request_id) = (request.pane_id.clone(), request.request_id.clone());
        let (declined, answered) = ((pane_id.clone(), request_id.clone()), (pane_id, request_id));
        let can_submit = !fields.is_empty();
        card(pane_interior::ROW_SPACING, palette)
            .child(heading(
                Icon::ClipboardList,
                palette.ink,
                request.title.clone(),
            ))
            .children(rows)
            .child(
                row()
                    .child(
                        button(
                            id("elicitation-cancel", &declined.1),
                            Role::Secondary,
                            elicitation::CANCEL,
                            palette,
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.answer_elicitation(&declined.0, &declined.1, false, cx)
                            },
                        )),
                    )
                    .child(div().flex_1())
                    .child(
                        button(
                            id("elicitation-submit", &answered.1),
                            Role::Primary,
                            elicitation::SUBMIT,
                            palette,
                        )
                        .when(!can_submit, |submit| {
                            submit.opacity(pane_interior::DISABLED_OPACITY)
                        })
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                if can_submit {
                                    this.answer_elicitation(&answered.0, &answered.1, true, cx);
                                }
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    /// The messages waiting for the agent, with Resume while the queue is paused.
    pub(super) fn queue_card(
        &mut self,
        pane_id: &str,
        state: &CardState,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !QueuePresentation::shown(&state.queue, state.queue_paused) {
            return None;
        }
        let queue = QueuePresentation::new(&state.queue, state.queue_paused);
        let resume = pane_id.to_owned();
        let header = row()
            .gap(px(pane_interior::COMPACT_SPACING))
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(pane_interior::CAPTION_POINT_SIZE))
            .child(
                row()
                    .flex_1()
                    .min_w_0()
                    .gap(px(space::X6))
                    .child(icon(
                        if queue.paused {
                            Icon::Paused
                        } else {
                            Icon::QueueEnd
                        },
                        desktop::INLINE_GLYPH,
                        palette.ink,
                    ))
                    .child(SharedString::from(queue.summary.clone())),
            )
            .children(queue.paused.then(|| {
                button(
                    id("queue-resume", &resume),
                    Role::Primary,
                    queue_words::RESUME,
                    palette,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.resume_queue(&resume)))
            }));
        let rows = queue.rows.iter().map(|queued| {
            let (pane_id, prompt_id) = (pane_id.to_owned(), queued.prompt_id.clone());
            row()
                .gap(px(pane_interior::COMPACT_SPACING))
                .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                .child(
                    column()
                        .flex_1()
                        .min_w_0()
                        .gap(px(space::X2))
                        .children(queued.sender.clone().map(|sender| {
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(SharedString::from(sender))
                        }))
                        .child(
                            div()
                                .line_clamp(2)
                                .child(SharedString::from(queued.text.clone())),
                        )
                        .children(queued.attachment_count.map(|count| {
                            row()
                                .gap(px(space::X6))
                                .child(icon(Icon::Paperclip, desktop::INLINE_GLYPH, palette.ink))
                                .child(count.to_string())
                        })),
                )
                .child(
                    div()
                        .id(id("queue-send-now", &queued.prompt_id))
                        .flex_none()
                        .text_color(rgb(palette.secondary))
                        .opacity(if queue_words::SEND_NOW_AVAILABLE {
                            1.0
                        } else {
                            pane_interior::DISABLED_OPACITY
                        })
                        .tooltip(super::ui::hint(queue_words::SEND_NOW_HINT, palette))
                        .child(queue_words::SEND_NOW),
                )
                .child(
                    row()
                        .id(id("queue-remove", &prompt_id))
                        .flex_none()
                        .cursor_pointer()
                        .own_press()
                        .tooltip(super::ui::hint(queue_words::REMOVE, palette))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                            this.remove_queued(&pane_id, &prompt_id)
                        }))
                        .child(icon(Icon::Close, desktop::INLINE_GLYPH, palette.secondary)),
                )
        });
        Some(
            card(pane_interior::COMPACT_SPACING, palette)
                .child(header)
                .children(rows)
                .into_any_element(),
        )
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

    /// The transcript's entries, with each subagent's activity under its row, and the line that
    /// says the parent waits on its subagents. The entries far from the viewport are spacers
    /// (see `transcript_rows`).
    pub(super) fn transcript_entries(
        &self,
        shown: &[soyeht_protocol::TranscriptMessage],
        markdown: &mut markdown::Cache,
        windowing: Windowing<'_>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let entries = cards::entries(shown);
        let keys: Vec<(String, u64)> = entries
            .iter()
            .map(|entry| {
                (
                    entry.id(),
                    transcript_rows::fingerprint(entry, self.open_state(windowing.pane_id, entry)),
                )
            })
            .collect();
        let plan = windowing.rows.borrow_mut().plan(
            &keys,
            windowing.viewport,
            pane_interior::STACK_SPACING,
            self.layout_signature,
        );
        let mut children = vec![Child::Other; windowing.leading];
        let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len() + 1);
        for ((entry, (key, fingerprint)), spacer) in entries.iter().zip(keys).zip(plan) {
            let row = match spacer {
                Some(height) => Some(spacer_row(height)),
                None => self.entry_row(windowing.pane_id, entry, markdown, palette, cx),
            };
            if let Some(row) = row {
                children.push(Child::Row {
                    key,
                    fingerprint,
                    built: spacer.is_none(),
                });
                rows.push(row);
            } else {
                windowing.rows.borrow_mut().mark_absent(&key, fingerprint);
            }
        }
        let waiting = cards::awaited(&entries);
        if waiting > 0 {
            children.push(Child::Other);
            rows.push(caption(palette.secondary, cards::waiting(waiting)).into_any_element());
        }
        windowing.rows.borrow_mut().set_children(children);
        rows
    }

    /// What an entry shows that its content does not hold: whether its thought or subagent is
    /// open, which changes its height.
    fn open_state(&self, pane_id: &str, entry: &Entry<'_>) -> u64 {
        match entry {
            Entry::Message(message) => {
                u64::from(self.panes.thought_is_open(pane_id, &message.local_id))
            }
            Entry::Subagent(group) => group.entries.iter().fold(
                u64::from(self.panes.subagent_is_open(pane_id, group.subagent_id)),
                |state, inner| {
                    state
                        .wrapping_mul(31)
                        .wrapping_add(self.open_state(pane_id, inner) + 1)
                },
            ),
        }
    }

    fn entry_row(
        &self,
        pane_id: &str,
        entry: &Entry<'_>,
        markdown: &mut markdown::Cache,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match entry {
            Entry::Message(message) => {
                let open = self.panes.thought_is_open(pane_id, &message.local_id);
                transcript_row(pane_id, message, open, markdown, palette, cx)
            }
            Entry::Subagent(group) => {
                Some(self.subagent_row(pane_id, group, markdown, palette, cx))
            }
        }
    }

    fn subagent_row(
        &self,
        pane_id: &str,
        group: &Group<'_>,
        markdown: &mut markdown::Cache,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let presentation = SubagentPresentation::new(group.run, timestamp::now());
        let expanded = self.panes.subagent_is_open(pane_id, group.subagent_id);
        let (owner, toggled) = (pane_id.to_owned(), group.subagent_id.to_owned());
        let header = column()
            .gap(px(space::X3))
            .child(
                row()
                    .gap(px(space::X8))
                    .child(disclosure_chevron(expanded, palette.secondary))
                    .child(status_mark(presentation.status, palette))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(SharedString::from(presentation.name.clone())),
                    )
                    .children(presentation.model.clone().map(|model| {
                        div()
                            .truncate()
                            .text_color(rgb(palette.secondary))
                            .child(SharedString::from(format!("\u{b7} {model}")))
                    }))
                    .child(div().flex_1().min_w(px(space::X8)))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                            .text_color(rgb(palette.secondary))
                            .child(SharedString::from(presentation.trailing.clone())),
                    ),
            )
            .children(match (&presentation.summary, expanded) {
                (Some(summary), false) => Some(
                    caption(palette.secondary, summary.clone())
                        .line_clamp(2)
                        .pl(px(desktop::SUBAGENT_SUMMARY_INDENT)),
                ),
                _ => None,
            });
        let body = expanded.then(|| {
            let task = group
                .run
                .map(|run| run.task.clone())
                .filter(|task| !task.is_empty());
            let children: Vec<AnyElement> = group
                .entries
                .iter()
                .filter_map(|entry| self.entry_row(pane_id, entry, markdown, palette, cx))
                .collect();
            column()
                .gap(px(space::X8))
                .pl(px(desktop::SUBAGENT_INDENT))
                .children(task.map(|task| caption(palette.secondary, task)))
                .children(children)
        });
        column()
            .gap(px(space::X8))
            .child(
                div()
                    .id(id("subagent", group.subagent_id))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_subagent(&owner, &toggled, cx)
                    }))
                    .child(header),
            )
            .children(body)
            .into_any_element()
    }

    /// "Load earlier messages": shows the messages the view holds beyond the page it draws, then
    /// asks the engine for the page before them.
    pub(super) fn load_earlier_button(
        &mut self,
        pane_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane_id.to_owned();
        row()
            .justify_center()
            .child(
                button(
                    id("load-earlier", &pane_id),
                    Role::Secondary,
                    cards::LOAD_EARLIER,
                    palette,
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.load_earlier(&pane_id, cx)),
                ),
            )
            .into_any_element()
    }
}

/// The mark in front of a subagent's name: a turning arrow while it runs, a check when it
/// succeeded, a cross when it failed or stopped.
fn status_mark(status: Status, palette: &Palette) -> AnyElement {
    match status {
        Status::Running => icon(Icon::Refresh, desktop::INLINE_GLYPH, palette.accent)
            .with_animation(
                "subagent-running",
                Animation::new(Duration::from_secs(1)).repeat(),
                |mark, delta| mark.with_transformation(Transformation::rotate(percentage(delta))),
            )
            .into_any_element(),
        Status::Succeeded => {
            icon(Icon::Check, desktop::INLINE_GLYPH, palette.success()).into_any_element()
        }
        Status::Failed => {
            icon(Icon::Close, desktop::INLINE_GLYPH, palette.danger()).into_any_element()
        }
        Status::Stopped => {
            icon(Icon::Close, desktop::INLINE_GLYPH, palette.secondary).into_any_element()
        }
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
