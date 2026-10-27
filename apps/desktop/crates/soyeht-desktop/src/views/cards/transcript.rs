//! The transcript's rows: its entries, the subagent rows under them and "Load earlier messages".

use super::super::{
    agent::{MessageRow, row_weight, transcript_row},
    markdown::Replies,
    ui::disclosure_chevron,
};
use super::{Role, button, caption};
use crate::{
    theme::{Palette, desktop, pane_interior, space},
    views::prelude::*,
};
use gpui::{Animation, AnimationExt, Transformation, percentage};
use soyeht_client::{
    agent::AgentView,
    cards::{self, Entry, Group, Status, SubagentPresentation},
    timestamp,
    transcript_rows::{self, Child, Key, Rows, Viewport},
};
use soyeht_protocol::TranscriptMessage;
use std::{cell::RefCell, time::Duration};

/// What a frame knows about the rows of the transcript it is building.
pub(in crate::views) struct Windowing<'a> {
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

impl Workbench {
    /// The transcript's entries, with each subagent's activity under its row, and the line that
    /// says the parent waits on its subagents. The entries far from the viewport are spacers
    /// (see `transcript_rows`).
    pub(in crate::views) fn transcript_entries(
        &self,
        view: &AgentView,
        limit: usize,
        markdown: &mut Replies,
        windowing: Windowing<'_>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let entries = cards::entries(view.tail(limit).0);
        let keys: Vec<Key> = entries
            .iter()
            .map(|entry| Key {
                key: entry.id(),
                fingerprint: transcript_rows::entry_fingerprint(
                    entry,
                    view,
                    self.open_state(windowing.pane_id, entry),
                ),
                weight: self.entry_weight(windowing.pane_id, entry, markdown),
            })
            .collect();
        let plan = windowing.rows.borrow_mut().plan(
            &keys,
            windowing.viewport,
            pane_interior::STACK_SPACING,
            self.stage.layout_signature,
        );
        let mut children = vec![Child::Other; windowing.leading];
        let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len() + 1);
        for (
            (
                entry,
                Key {
                    key, fingerprint, ..
                },
            ),
            spacer,
        ) in entries.iter().zip(keys).zip(plan)
        {
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
    /// open and how many pages of its reply are drawn, which change its height.
    fn open_state(&self, pane_id: &str, entry: &Entry<'_>) -> u64 {
        match entry {
            Entry::Message(message) => {
                u64::from(self.panes.thought_is_open(pane_id, &message.local_id))
                    + 2 * self.panes.reply_pages(pane_id, &message.local_id) as u64
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

    /// What building an entry costs (see `agent::row_weight`); a subagent row weighs its own line
    /// and, when it is open, what is under it.
    fn entry_weight(&self, pane_id: &str, entry: &Entry<'_>, markdown: &mut Replies) -> usize {
        let Entry::Subagent(group) = entry else {
            let Entry::Message(message) = entry else {
                return 0;
            };
            return row_weight(self.message_row(pane_id, message), markdown);
        };
        let mut weight = 1;
        if self.panes.subagent_is_open(pane_id, group.subagent_id) {
            for inner in &group.entries {
                weight += self.entry_weight(pane_id, inner, markdown);
            }
        }
        weight
    }

    /// A message with whether its thought is open and how many pages of its reply are drawn.
    fn message_row<'a>(&self, pane_id: &'a str, message: &'a TranscriptMessage) -> MessageRow<'a> {
        MessageRow {
            pane_id,
            message,
            open: self.panes.thought_is_open(pane_id, &message.local_id),
            pages: self.panes.reply_pages(pane_id, &message.local_id),
        }
    }

    fn entry_row(
        &self,
        pane_id: &str,
        entry: &Entry<'_>,
        markdown: &mut Replies,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match entry {
            Entry::Message(message) => {
                transcript_row(self.message_row(pane_id, message), markdown, palette, cx)
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
        markdown: &mut Replies,
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
    pub(in crate::views) fn load_earlier_button(
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
