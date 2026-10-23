//! The messages waiting for the agent.

use super::{Role, button, card};
use crate::{
    theme::{Palette, desktop, pane_interior, space},
    views::prelude::*,
};
use soyeht_client::cards::{CardState, QueuePresentation, queue as queue_words};

impl Workbench {
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
                        .tooltip(crate::views::ui::hint(queue_words::SEND_NOW_HINT, palette))
                        .child(queue_words::SEND_NOW),
                )
                .child(
                    row()
                        .id(id("queue-remove", &prompt_id))
                        .flex_none()
                        .cursor_pointer()
                        .own_press()
                        .tooltip(crate::views::ui::hint(queue_words::REMOVE, palette))
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
}
