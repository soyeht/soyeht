//! The inside of an agent pane, in the approved flat interior: a person's message is a `canvas`
//! card headed "YOU", the agent's reply is plain text, a tool call is a plain row over a thin
//! line, and the composer sits at the bottom. Nothing in here has a shadow.

use super::{
    attachments::sent_chips,
    cards::{Role, Windowing, button, caption},
    copy_button::copyable,
    markdown::{Opens, Replies},
    selectable::Selectable,
    ui::{PaneScroll, Typeface, code_well, column, disclosure_chevron, icon, id, row},
};
use crate::{
    assets::Icon,
    theme::{Palette, desktop, pane_interior, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, ExternalPaths, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px, rgb,
};
use soyeht_client::{
    agent::{owner_of, terminal_text},
    markdown::{EARLIER_HIDDEN, SHOW_MORE, Shown, text_weight},
    transcript_rows::Viewport,
};
use soyeht_protocol::{
    PlanStatus, TerminalOutput, ToolCall, ToolStatus, TranscriptKind, TranscriptMessage,
    TranscriptRole,
};

/// What a message row is drawn from: the message, whether its thought is open, and how many pages
/// of its reply are drawn.
#[derive(Clone, Copy)]
pub(super) struct MessageRow<'a> {
    pub pane_id: &'a str,
    pub message: &'a TranscriptMessage,
    pub open: bool,
    pub pages: usize,
}

/// How far from the end of the transcript, in pixels, still counts as at the newest message.
const PINNED: f32 = 2.0;

impl Workbench {
    pub(super) fn agent_body(
        &mut self,
        pane: &soyeht_protocol::Pane,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = self
            .state
            .agent(&pane.pane_id)
            .map_or(0, |view| view.messages().len());
        let (scroll, follows) = self.transcript_scroll(&pane.pane_id, count);
        let measured = self.transcript_rows(&pane.pane_id);
        // A wheel can leave the offset past the ends until the next layout clamps it.
        let from_bottom = {
            let max = f32::from(scroll.max_offset().y);
            (max + f32::from(scroll.offset().y)).clamp(0.0, max)
        };
        let pinned = follows || from_bottom < PINNED;
        let viewport = Viewport {
            // The area was laid out before this frame (zero before the first layout). The scroll
            // to the newest message lands in this frame's layout, after the rows have been
            // chosen, so a frame that follows plans for the bottom of the transcript.
            height: f32::from(scroll.bounds().size.height),
            from_bottom: if follows { 0.0 } else { from_bottom },
        };
        let limit = self.transcript_limit(&pane.pane_id);
        // The transcript is read where it lives and only the elements are built: the last
        // `limit` messages, never a copy of all of them. The Markdown cache is taken out of
        // the workbench while the rows are built, so the transcript is only read.
        let mut markdown = std::mem::take(&mut self.markdown);
        markdown.files = self.files_of(&pane.pane_id, markdown.probes.clone(), cx);
        let notice = self
            .state
            .agent(&pane.pane_id)
            .and_then(|view| view.notice.clone());
        let load_earlier = self
            .state
            .agent(&pane.pane_id)
            .is_some_and(|view| view.cards.has_earlier(view.tail(limit).1))
            .then(|| self.load_earlier_button(&pane.pane_id, palette, cx));
        let leading = usize::from(load_earlier.is_some()) + usize::from(notice.is_some());
        let rows = match self.state.agent(&pane.pane_id) {
            Some(view) => self.transcript_entries(
                view,
                limit,
                &mut markdown,
                Windowing {
                    pane_id: &pane.pane_id,
                    rows: &measured,
                    viewport,
                    leading,
                },
                palette,
                cx,
            ),
            None => Vec::new(),
        };
        self.markdown = markdown;
        // Rows that were never measured are measured a budget at a time, and the transcript
        // stays at the newest message while they change the height above it.
        if measured.borrow().measuring() && pinned {
            scroll.scroll_to_bottom();
        }
        if measured.borrow().catching_up() {
            cx.notify();
        }
        let dropped = pane.pane_id.clone();
        column()
            .flex_1()
            .min_h_0()
            // Files dropped anywhere on the conversation attach to the composer, and the pane
            // shows it will take them with an accent border, as the Mac's does.
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                this.attach_paths(&dropped, paths.paths().to_vec(), cx)
            }))
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style
                    .border(px(desktop::DROP_BORDER))
                    .border_color(rgb(palette.accent))
                    .rounded(px(pane_interior::CARD_RADIUS))
            })
            .child(
                div()
                    .id(id("transcript", &pane.pane_id))
                    .flex_1()
                    .min_h_0()
                    .pane_scroll_y()
                    .track_scroll(&scroll)
                    .px(px(pane_interior::CARD_PADDING))
                    .py(px(pane_interior::CARD_VERTICAL_PADDING))
                    .child(
                        column()
                            .gap(px(pane_interior::STACK_SPACING))
                            // The sizes the rows were laid out at tell the next frame which of
                            // them it can stand a spacer in for.
                            .on_children_prepainted(move |bounds, _, _| {
                                let sizes: Vec<(f32, f32)> = bounds
                                    .iter()
                                    .map(|bounds| {
                                        (
                                            f32::from(bounds.size.width),
                                            f32::from(bounds.size.height),
                                        )
                                    })
                                    .collect();
                                measured.borrow_mut().absorb(&sizes);
                            })
                            .children(load_earlier)
                            .children(notice.as_deref().map(|text| notice_line(text, palette)))
                            .children(rows),
                    ),
            )
            .children(self.cards_under_transcript(pane, palette, cx))
            .child(self.composer(pane, palette, cx))
            .into_any_element()
    }
}

/// A flat pill: the shape of Allow, Deny and every other button inside a pane.
pub(super) fn pill(key: SharedString, fill: u32, ink: u32) -> gpui::Stateful<Div> {
    div()
        .id(key)
        .px(px(pane_interior::PILL_HORIZONTAL_PADDING))
        .py(px(pane_interior::PILL_VERTICAL_PADDING))
        .rounded_full()
        .bg(rgb(fill))
        .text_color(rgb(ink))
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::CONTROL))
        .cursor_pointer()
        .active(|style| style.opacity(pane_interior::PRESSED_OPACITY))
}

/// A caption with the small icon the Mac's `Label` puts before it.
pub(super) fn labelled(glyph: Icon, color: u32, text: impl Into<SharedString>) -> Div {
    row()
        .gap(px(space::X6))
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(color))
        .child(icon(glyph, desktop::INLINE_GLYPH, color))
        .child(div().flex_1().min_w_0().child(text.into()))
}

pub(super) fn notice_line(text: &str, palette: &Palette) -> AnyElement {
    labelled(Icon::Alert, palette.warning(), text.to_owned()).into_any_element()
}

pub(super) fn transcript_row(
    line: MessageRow,
    markdown: &mut Replies,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> Option<AnyElement> {
    let MessageRow {
        pane_id,
        message,
        open,
        ..
    } = line;
    match message.kind {
        TranscriptKind::Tool => message.tool.as_ref().map(|tool| tool_row(tool, palette)),
        TranscriptKind::Thought => {
            let body = open.then(|| markdown_body(line, markdown, palette, palette.secondary, cx));
            Some(thought_row(pane_id, message, body, palette, cx))
        }
        TranscriptKind::Plan => message.plan.as_ref().map(|plan| plan_card(plan, palette)),
        TranscriptKind::Handover => Some(
            labelled(
                Icon::Refresh,
                palette.secondary,
                format!("Conversation continued with {}", message.text),
            )
            .into_any_element(),
        ),
        TranscriptKind::Terminal => message
            .terminal
            .as_ref()
            .map(|output| terminal_block(&owner_of(pane_id, &message.local_id), output, palette)),
        // A subagent's row is drawn from its group, with the activity it owns under it.
        TranscriptKind::Usage | TranscriptKind::Subagent => None,
        TranscriptKind::Message | TranscriptKind::System => {
            Some(text_row(line, palette, markdown, cx))
        }
    }
}

/// What laying out `message` costs, in the weight of `markdown::weight_of`: the reply as far as
/// its pages go, the text of anything else.
pub(super) fn row_weight(line: MessageRow, markdown: &mut Replies) -> usize {
    let message = line.message;
    match message.kind {
        TranscriptKind::Thought if line.open => shown_reply(line, markdown).weight(),
        TranscriptKind::Message | TranscriptKind::System
            if message.role == TranscriptRole::Assistant && message.agent_message.is_none() =>
        {
            shown_reply(line, markdown).weight()
        }
        TranscriptKind::Message | TranscriptKind::System => text_weight(message.text.len()),
        TranscriptKind::Terminal => message
            .terminal
            .as_ref()
            .map_or(1, |output| text_weight(output.bytes.len() / 4 * 3)),
        TranscriptKind::Plan => message.plan.as_ref().map_or(1, |plan| 1 + plan.len()),
        TranscriptKind::Usage | TranscriptKind::Subagent => 0,
        TranscriptKind::Tool | TranscriptKind::Thought | TranscriptKind::Handover => 1,
    }
}

/// The pages of a message's reply that are drawn, named by the message (`owner_of`).
fn shown_reply(line: MessageRow, markdown: &mut Replies) -> Shown {
    let owner = owner_of(line.pane_id, &line.message.local_id);
    markdown.shown(
        &owner,
        &line.message.text,
        !line.message.complete,
        line.pages,
    )
}

/// The agent's words with their Markdown drawn, in `ink`; a reply that is still arriving has its
/// open spans closed. Its blocks are named by the message (`owner_of`), so a selection or a copy
/// button of one reply is not another's. A long reply shows `pages` pages and a row that asks
/// for the next one (`markdown::shown`).
fn markdown_body(
    line: MessageRow,
    markdown: &mut Replies,
    palette: &'static Palette,
    ink: u32,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let owner = owner_of(line.pane_id, &line.message.local_id);
    let shown = shown_reply(line, markdown);
    let reply = super::markdown::reply(
        &shown.blocks,
        &owner,
        palette,
        ink,
        Opens::Links(markdown.files.as_ref()),
    );
    if !shown.more {
        return reply;
    }
    if shown.tail {
        // The end of a reply that is arriving: no row to ask for more, the beginning comes with
        // the whole reply.
        return column()
            .gap(px(pane_interior::COMPACT_SPACING))
            .child(caption(palette.secondary, EARLIER_HIDDEN))
            .child(reply)
            .into_any_element();
    }
    let (pane_id, message_id) = (line.pane_id.to_owned(), line.message.local_id.clone());
    let more = id("show-more", &message_id);
    let ask = cx.listener(move |this, _: &ClickEvent, _, cx| {
        this.show_more_of_reply((&pane_id, &message_id), cx)
    });
    column()
        .gap(px(pane_interior::COMPACT_SPACING))
        .child(reply)
        .child(row().child(button(more, Role::Secondary, SHOW_MORE, palette).on_click(ask)))
        .into_any_element()
}

/// A message: a card for the person or a peer agent, plain text for what they typed, and the
/// agent's own words with their Markdown drawn.
fn text_row(
    line: MessageRow,
    palette: &'static Palette,
    markdown: &mut Replies,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let (pane_id, message) = (line.pane_id, line.message);
    let owner = owner_of(pane_id, &message.local_id);
    let copyable = |card: Div| {
        copyable(
            card,
            &format!("message-{owner}"),
            message.text.clone(),
            pane_interior::TIGHT_SPACING,
            palette,
        )
    };
    if message.role == TranscriptRole::Assistant && message.agent_message.is_none() {
        let reply = div()
            .w_full()
            .text_size(px(pane_interior::BODY_POINT_SIZE))
            .child(markdown_body(line, markdown, palette, palette.ink, cx));
        return copyable(reply).into_any_element();
    }
    let body = div()
        .text_size(px(pane_interior::BODY_POINT_SIZE))
        .child(Selectable::plain(
            format!("message-{owner}"),
            message.text.clone(),
            palette,
        ));
    if message.role == TranscriptRole::User || message.agent_message.is_some() {
        let sender = message
            .agent_message
            .as_ref()
            .map(|peer| {
                if peer.name.trim().is_empty() {
                    peer.harness.clone()
                } else {
                    peer.name.clone()
                }
            })
            .unwrap_or_else(|| "You".to_owned());
        let card = column()
            .gap(px(pane_interior::COMPACT_SPACING))
            .px(px(pane_interior::CARD_PADDING))
            .py(px(pane_interior::CARD_VERTICAL_PADDING))
            .rounded(px(pane_interior::CARD_RADIUS))
            .bg(rgb(palette.canvas))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                    .text_color(rgb(if message.agent_message.is_some() {
                        palette.accent
                    } else {
                        palette.secondary
                    }))
                    .child(SharedString::from(sender.to_uppercase())),
            )
            .child(body)
            .children(
                message
                    .attachments
                    .iter()
                    .filter(|attachments| !attachments.is_empty())
                    .map(|attachments| sent_chips(attachments, palette)),
            );
        return copyable(card).into_any_element();
    }
    body.text_color(rgb(palette.secondary)).into_any_element()
}

/// A tool call as a plain row: kind, title and paths in mono, the status at the trailing edge,
/// a thin line under it.
fn tool_row(tool: &ToolCall, palette: &Palette) -> AnyElement {
    let (status, color) = match tool.status {
        ToolStatus::Pending => ("pending", palette.secondary),
        ToolStatus::InProgress => ("\u{25cf} running", palette.accent),
        ToolStatus::Completed => ("\u{2713} done", palette.success()),
        ToolStatus::Failed => ("\u{2715} failed", palette.danger()),
    };
    let locations = tool.locations.iter().map(|location| {
        let text = match location.line {
            Some(line) => format!("{}:{}", location.path, u64::from(line) + 1),
            None => location.path.clone(),
        };
        div()
            .mono()
            .text_size(px(pane_interior::MONO_SMALL_POINT_SIZE))
            .text_color(rgb(palette.secondary))
            .child(SharedString::from(text))
    });
    row()
        .items_start()
        .gap(px(pane_interior::ROW_SPACING))
        .py(px(pane_interior::ROW_VERTICAL_PADDING))
        .border_b_1()
        .border_color(rgb(palette.well))
        .mono()
        .text_size(px(pane_interior::MONO_POINT_SIZE))
        .child(
            column()
                .flex_1()
                .min_w_0()
                .gap(px(pane_interior::TIGHT_SPACING))
                .child(
                    row()
                        .items_start()
                        .gap(px(pane_interior::ROW_SPACING))
                        .children(
                            tool.kind
                                .as_ref()
                                .filter(|kind| !kind.is_empty())
                                .map(|kind| {
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(palette.ink))
                                        .child(SharedString::from(capitalized(kind)))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .line_clamp(2)
                                .text_color(rgb(palette.secondary))
                                .child(SharedString::from(tool.title.clone())),
                        ),
                )
                .children(locations),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(pane_interior::MONO_SMALL_POINT_SIZE))
                .text_color(rgb(color))
                .child(status),
        )
        .into_any_element()
}

/// A thought: collapsed to its name until the person opens it, as on the Mac. `body` is what an
/// open thought shows.
fn thought_row(
    pane_id: &str,
    message: &TranscriptMessage,
    body: Option<AnyElement>,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let open = body.is_some();
    let (pane_id, message_id) = (pane_id.to_owned(), message.local_id.clone());
    column()
        .gap(px(pane_interior::COMPACT_SPACING))
        .text_size(px(pane_interior::BODY_POINT_SIZE))
        .text_color(rgb(palette.secondary))
        .child(
            row()
                .id(id("thought", &message.local_id))
                .gap(px(space::X6))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_thought(&pane_id, &message_id, cx)
                }))
                .child(disclosure_chevron(open, palette.secondary))
                .child("Thought"),
        )
        .children(body)
        .into_any_element()
}

/// A command's output: the text in a mono well, as the Mac shows it.
fn terminal_block(owner: &str, output: &TerminalOutput, palette: &'static Palette) -> AnyElement {
    let text = terminal_text(output);
    copyable(
        code_well(
            Selectable::plain(format!("terminal-{owner}"), text.clone(), palette),
            palette,
        )
        .w_full(),
        &format!("terminal-{owner}"),
        text,
        pane_interior::CARD_PADDING,
        palette,
    )
    .into_any_element()
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect()
    })
}

/// The plan: its name with the clipboard icon, then each step with a filled mark when done and an
/// empty circle while it is not.
fn plan_card(plan: &[soyeht_protocol::PlanEntry], palette: &Palette) -> AnyElement {
    column()
        .gap(px(pane_interior::COMPACT_SPACING))
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .child(
            row()
                .gap(px(space::X6))
                .font_weight(FontWeight::SEMIBOLD)
                .child(icon(
                    Icon::ClipboardList,
                    desktop::INLINE_GLYPH,
                    palette.ink,
                ))
                .child("Plan"),
        )
        .children(plan.iter().map(|entry| {
            let mark = if entry.status == PlanStatus::Completed {
                div()
                    .size(px(desktop::INLINE_GLYPH))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(palette.ink))
                    .child(icon(Icon::Check, desktop::CHECK_GLYPH, palette.face))
                    .into_any_element()
            } else {
                icon(Icon::Circle, desktop::INLINE_GLYPH, palette.ink).into_any_element()
            };
            row()
                .gap(px(space::X6))
                .child(mark)
                .child(div().child(SharedString::from(entry.content.clone())))
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_kinds_are_capitalized_as_on_the_mac() {
        assert_eq!(capitalized("execute"), "Execute");
        assert_eq!(capitalized("READ"), "Read");
        assert_eq!(capitalized(""), "");
    }
}
