//! The question the agent waits on.

use super::{Role, button, card, heading};
use crate::{
    theme::{Palette, pane_interior},
    views::prelude::*,
};
use soyeht_client::cards::elicitation;
use soyeht_protocol::ElicitationRequest;

impl Workbench {
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
}
