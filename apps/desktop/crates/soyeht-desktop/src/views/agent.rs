//! The inside of an agent pane, in the approved flat interior: a person's message is a `canvas`
//! card headed "YOU", the agent's reply is plain text, a tool call is a plain row over a thin
//! line, and the composer sits at the bottom. Nothing in here has a shadow.

use super::ui::{column, id, row};
use crate::{
    theme::{Palette, fonts, pane_interior, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px, rgb,
};
use soyeht_protocol::{
    PermissionOptionKind, PermissionRequest, PlanStatus, ToolCall, ToolStatus, TranscriptKind,
    TranscriptMessage, TranscriptRole,
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
        let (rows, hidden, notice, permission) = match self.state.agent(&pane.pane_id) {
            Some(view) => {
                let (shown, hidden) = view.tail();
                let rows: Vec<AnyElement> = shown
                    .iter()
                    .filter(|message| message.subagent_id.is_none())
                    .filter_map(|message| {
                        let open = self.open_thoughts.contains(&message.local_id);
                        transcript_row(message, open, palette, cx)
                    })
                    .collect();
                let permission = view
                    .permission
                    .as_ref()
                    .map(|request| permission_card(&pane.pane_id, request, palette, cx));
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

/// The agent asks to run something: the request in a flat card, Allow as the primary pill.
fn permission_card(
    pane_id: &str,
    request: &PermissionRequest,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let buttons: Vec<AnyElement> = request
        .options
        .iter()
        .map(|option| {
            let (pane_id, request_id, option_id) = (
                pane_id.to_owned(),
                request.request_id.clone(),
                option.option_id.clone(),
            );
            let (fill, ink) = match option.kind {
                PermissionOptionKind::AllowOnce => (palette.ink, palette.face),
                PermissionOptionKind::AllowAlways => (palette.well, palette.ink),
                PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways => {
                    (palette.well, palette.danger())
                }
            };
            pill(id("permission", &option.option_id), fill, ink)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.answer_permission(&pane_id, &request_id, &option_id)
                }))
                .child(SharedString::from(option.name.clone()))
                .into_any_element()
        })
        .collect();
    column()
        .gap(px(pane_interior::ROW_SPACING))
        .p(px(pane_interior::CARD_PADDING))
        .rounded(px(pane_interior::CARD_RADIUS))
        .bg(rgb(palette.canvas))
        .child(
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_size(px(pane_interior::HEADING_POINT_SIZE))
                .child(SharedString::from(request.title.clone())),
        )
        .children((!request.detail.is_empty()).then(|| {
            div()
                .text_color(rgb(palette.secondary))
                .child(SharedString::from(request.detail.clone()))
        }))
        .child(
            row()
                .gap(px(pane_interior::COMPACT_SPACING))
                .children(buttons),
        )
        .into_any_element()
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

fn notice_line(text: &str, palette: &Palette) -> AnyElement {
    div()
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(palette.warning()))
        .child(SharedString::from(format!("\u{26a0} {text}")))
        .into_any_element()
}

fn transcript_row(
    message: &TranscriptMessage,
    open: bool,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> Option<AnyElement> {
    match message.kind {
        TranscriptKind::Tool => message.tool.as_ref().map(|tool| tool_row(tool, palette)),
        TranscriptKind::Thought => Some(thought_row(message, open, palette, cx)),
        TranscriptKind::Plan => message.plan.as_ref().map(|plan| plan_card(plan, palette)),
        TranscriptKind::Handover => Some(
            div()
                .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                .text_color(rgb(palette.secondary))
                .child(SharedString::from(format!(
                    "Conversation continued with {}",
                    message.text
                )))
                .into_any_element(),
        ),
        TranscriptKind::Usage | TranscriptKind::Terminal | TranscriptKind::Subagent => None,
        TranscriptKind::Message | TranscriptKind::System => Some(text_row(message, palette)),
    }
}

/// A message: a card for the person or a peer agent, plain text for the agent's own words.
fn text_row(message: &TranscriptMessage, palette: &Palette) -> AnyElement {
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
    body.text_color(rgb(if message.role == TranscriptRole::System {
        palette.secondary
    } else {
        palette.ink
    }))
    .into_any_element()
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
            .font_family(fonts::MONO)
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
        .font_family(fonts::MONO)
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
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let message_id = message.local_id.clone();
    column()
        .gap(px(pane_interior::COMPACT_SPACING))
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(palette.secondary))
        .child(
            row()
                .id(id("thought", &message.local_id))
                .gap(px(pane_interior::TIGHT_SPACING))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_thought(&message_id, cx)
                }))
                .child(if open { "\u{25be}" } else { "\u{25b8}" })
                .child("Thought"),
        )
        .children(open.then(|| div().child(SharedString::from(message.text.clone()))))
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

fn plan_card(plan: &[soyeht_protocol::PlanEntry], palette: &Palette) -> AnyElement {
    column()
        .gap(px(pane_interior::COMPACT_SPACING))
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .child(div().font_weight(FontWeight::SEMIBOLD).child("Plan"))
        .children(plan.iter().map(|entry| {
            let mark = if entry.status == PlanStatus::Completed {
                "\u{2713}"
            } else {
                "\u{25cb}"
            };
            row()
                .gap(px(pane_interior::COMPACT_SPACING))
                .child(div().text_color(rgb(palette.secondary)).child(mark))
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
