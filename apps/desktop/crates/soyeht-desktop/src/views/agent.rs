//! The inside of an agent pane, in the approved flat interior: a person's message is a `canvas`
//! card headed "YOU", the agent's reply is plain text, a tool call is a plain row over a thin
//! line, and the composer sits at the bottom. Nothing in here has a shadow.

use super::ui::{Typeface, code_well, column, icon, id, row};
use crate::{
    assets::Icon,
    theme::{Palette, desktop, pane_interior, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px, rgb,
};
use soyeht_client::{agent::terminal_text, markdown};
use soyeht_protocol::{
    PlanStatus, TerminalOutput, ToolCall, ToolStatus, TranscriptKind, TranscriptMessage,
    TranscriptRole,
};

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
            .map_or(0, |view| view.messages.len());
        let scroll = self.transcript_scroll(&pane.pane_id, count);
        // The transcript is read where it lives and only the elements are built: the last
        // `TAIL` messages, never a copy of all of them.
        let markdown = &mut self.markdown;
        let (rows, hidden, notice, permission) = match self.state.agent(&pane.pane_id) {
            Some(view) => {
                let (shown, hidden) = view.tail();
                let rows: Vec<AnyElement> = shown
                    .iter()
                    .filter(|message| message.subagent_id.is_none())
                    .filter_map(|message| {
                        let open = self.open_thoughts.contains(&message.local_id);
                        transcript_row(message, open, markdown, palette, cx)
                    })
                    .collect();
                let permission = view
                    .permission
                    .as_ref()
                    .map(|request| super::permission::card(&pane.pane_id, request, palette, cx));
                (rows, hidden, view.notice.clone(), permission)
            }
            None => (Vec::new(), 0, None, None),
        };
        column()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id(id("transcript", &pane.pane_id))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .px(px(pane_interior::CARD_PADDING))
                    .py(px(pane_interior::CARD_VERTICAL_PADDING))
                    .child(
                        column()
                            .gap(px(pane_interior::STACK_SPACING))
                            .children(notice.as_deref().map(|text| notice_line(text, palette)))
                            .children((hidden > 0).then(|| earlier_line(hidden, palette)))
                            .children(rows),
                    ),
            )
            .children(permission.map(|card| {
                div()
                    .px(px(pane_interior::CARD_PADDING))
                    .pb(px(pane_interior::ROW_SPACING))
                    .child(card)
            }))
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

fn earlier_line(hidden: usize, palette: &Palette) -> AnyElement {
    div()
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(palette.secondary))
        .child(SharedString::from(format!(
            "{hidden} earlier message{} not shown",
            if hidden == 1 { "" } else { "s" }
        )))
        .into_any_element()
}

/// A caption with the small icon the Mac's `Label` puts before it.
fn labelled(glyph: Icon, color: u32, text: impl Into<SharedString>) -> Div {
    row()
        .gap(px(space::X6))
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(color))
        .child(icon(glyph, desktop::INLINE_GLYPH, color))
        .child(div().flex_1().min_w_0().child(text.into()))
}

fn notice_line(text: &str, palette: &Palette) -> AnyElement {
    labelled(Icon::Alert, palette.warning(), text.to_owned()).into_any_element()
}

fn transcript_row(
    message: &TranscriptMessage,
    open: bool,
    markdown: &mut markdown::Cache,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> Option<AnyElement> {
    match message.kind {
        TranscriptKind::Tool => message.tool.as_ref().map(|tool| tool_row(tool, palette)),
        TranscriptKind::Thought => Some(thought_row(message, open, markdown, palette, cx)),
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
            .map(|output| terminal_block(output, palette)),
        TranscriptKind::Usage | TranscriptKind::Subagent => None,
        TranscriptKind::Message | TranscriptKind::System => {
            Some(text_row(message, markdown, palette))
        }
    }
}

/// The agent's words with their Markdown drawn, in `ink`; a reply that is still arriving has its
/// open spans closed.
fn markdown_body(
    message: &TranscriptMessage,
    markdown: &mut markdown::Cache,
    palette: &Palette,
    ink: u32,
) -> AnyElement {
    let blocks = markdown.blocks(&message.local_id, &message.text, !message.complete);
    super::markdown::reply(&blocks, &message.local_id, palette, ink)
}

/// A message: a card for the person or a peer agent, plain text for what they typed, and the
/// agent's own words with their Markdown drawn.
fn text_row(
    message: &TranscriptMessage,
    markdown: &mut markdown::Cache,
    palette: &Palette,
) -> AnyElement {
    if message.role == TranscriptRole::Assistant && message.agent_message.is_none() {
        return div()
            .w_full()
            .text_size(px(pane_interior::BODY_POINT_SIZE))
            .child(markdown_body(message, markdown, palette, palette.ink))
            .into_any_element();
    }
    let body = div()
        .text_size(px(pane_interior::BODY_POINT_SIZE))
        .child(SharedString::from(message.text.clone()));
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
        return column()
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
            .into_any_element();
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

/// A thought: collapsed to its name until the person opens it, as on the Mac.
fn thought_row(
    message: &TranscriptMessage,
    open: bool,
    markdown: &mut markdown::Cache,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let message_id = message.local_id.clone();
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
                    this.toggle_thought(&message_id, cx)
                }))
                .child(icon(
                    if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    desktop::DISCLOSURE_GLYPH,
                    palette.secondary,
                ))
                .child("Thought"),
        )
        .children(open.then(|| markdown_body(message, markdown, palette, palette.secondary)))
        .into_any_element()
}

/// A command's output: the text in a mono well, as the Mac shows it.
fn terminal_block(output: &TerminalOutput, palette: &Palette) -> AnyElement {
    code_well(terminal_text(output), palette)
        .w_full()
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
