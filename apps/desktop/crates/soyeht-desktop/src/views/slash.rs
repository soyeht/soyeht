//! The composer's "/" list: a flat card that floats above the composer card, as wide as it, with
//! a row for each skill and command that matches what is written. The chosen row sits on the
//! well; the keys and the choice are `slash_ui`'s, the rows and their order the client's.

use super::{cards::caption, prelude::*};
use crate::theme::{Palette, pane_interior};
use gpui::{Stateful, deferred};
use soyeht_client::slash::{Source, Suggestion};

impl Workbench {
    /// The list above the composer card of `pane_id`, to be a child of that (relative) card.
    /// Nothing while there are no rows.
    pub(super) fn slash_list(
        &mut self,
        rows: &[Suggestion],
        palette: &'static Palette,
        pane_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if rows.is_empty() {
            return None;
        }
        let chosen = self.slash_selection(pane_id, rows);
        let rows = rows.iter().enumerate().map(|(index, suggestion)| {
            let (owner, picked) = (pane_id.to_owned(), suggestion.clone());
            slash_row(suggestion, chosen == Some(index), palette)
                .id(id("slash-row", &format!("{pane_id}-{}", suggestion.name)))
                .cursor_pointer()
                .own_press()
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.choose_slash(&owner, &picked, cx);
                    this.focus_input(&owner, window, cx);
                }))
        });
        // Deferred, so that it paints over the transcript above the card; absolute, so that the
        // card's own height does not change.
        Some(deferred(
            column()
                .id(id("slash-list", pane_id))
                .absolute()
                .left_0()
                .right_0()
                .bottom_full()
                .mb(px(pane_interior::COMPACT_SPACING))
                .p(px(pane_interior::TIGHT_SPACING))
                .rounded(px(pane_interior::CARD_RADIUS))
                .bg(rgb(palette.face))
                .border(px(pane_interior::SEPARATOR_WIDTH))
                .border_color(rgb(palette.well))
                .occlude()
                .children(rows),
        ))
    }
}

/// One row: `/name`, what the command takes, its description, and "Skill" at the end for a
/// Soyeht skill.
fn slash_row(suggestion: &Suggestion, chosen: bool, palette: &Palette) -> Stateful<Div> {
    let skill = matches!(suggestion.source, Source::Skill(_));
    row()
        .id("slash-row")
        .gap(px(pane_interior::COMPACT_SPACING))
        .px(px(pane_interior::COMPACT_SPACING))
        .py(px(pane_interior::ROW_VERTICAL_PADDING))
        .rounded(px(pane_interior::WELL_RADIUS))
        .when_chosen(chosen, palette)
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.ink))
                .child(SharedString::from(format!("/{}", suggestion.name))),
        )
        .children(
            suggestion
                .hint
                .as_deref()
                .map(|hint| caption(palette.muted, hint.to_owned()).flex_none()),
        )
        .child(
            caption(palette.secondary, suggestion.detail.clone())
                .flex_1()
                .min_w_0()
                .truncate(),
        )
        .children(skill.then(|| caption(palette.accent, "Skill").flex_none()))
}

trait Chosen: Styled + Sized {
    /// The chosen row sits on the well.
    fn when_chosen(self, chosen: bool, palette: &Palette) -> Self {
        if chosen {
            self.bg(rgb(palette.well))
        } else {
            self
        }
    }
}

impl<T: Styled> Chosen for T {}
