//! Text of the transcript that can be selected with the pointer and copied, as the Mac's
//! `.textSelection(.enabled)` allows. A drag, a double click (a word) or a triple click (a line)
//! selects inside one block of text; the rule is `BlockSelection` of the client. The selection is
//! the window's: choosing in one block ends it in the other. Copy takes it from here
//! (`selected_text`), with the keys of any text field.

use crate::{pointer::follow, theme::Palette};
use gpui::{
    AnyElement, App, ElementId, Global, HighlightStyle, InteractiveElement, InteractiveText,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, RenderOnce,
    SharedString, Styled, StyledText, TextLayout, Window, canvas, div, rgb,
};
use soyeht_client::text_input::block::BlockSelection;
use std::ops::Range;

/// The window's selection in transcript text.
#[derive(Default)]
struct Selected(BlockSelection);

impl Global for Selected {}

/// The text selected in the transcript, when something is.
pub(crate) fn selected_text(cx: &App) -> Option<String> {
    cx.try_global::<Selected>()
        .and_then(|selected| selected.0.text())
        .map(str::to_owned)
}

/// Ends the selection in the transcript.
pub(crate) fn clear_selection(cx: &mut App) {
    if cx.has_global::<Selected>() {
        cx.global_mut::<Selected>().0.clear();
    }
}

/// A block of read-only text: `content` with the looks in `highlights` (ranges of bytes that do
/// not overlap, in order), the font of `mono` ranges and the addresses of `links`. `key` names
/// the block among all that can be selected.
#[derive(IntoElement)]
pub(super) struct Selectable {
    pub key: SharedString,
    pub content: SharedString,
    pub highlights: Vec<(Range<usize>, HighlightStyle)>,
    pub mono: Vec<(Range<usize>, SharedString)>,
    pub links: Vec<(Range<usize>, String)>,
    pub palette: &'static Palette,
}

impl Selectable {
    pub fn plain(
        key: impl Into<SharedString>,
        content: impl Into<SharedString>,
        palette: &'static Palette,
    ) -> Self {
        Self {
            key: key.into(),
            content: content.into(),
            highlights: Vec::new(),
            mono: Vec::new(),
            links: Vec::new(),
            palette,
        }
    }
}

/// `looks` with the background of `shade` over `selected`: the pieces of a look that the
/// selection cuts are cut with it, and the part of the selection with no look has only the shade.
fn shaded(
    looks: Vec<(Range<usize>, HighlightStyle)>,
    selected: Range<usize>,
    shade: HighlightStyle,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut cuts = vec![selected.start, selected.end];
    for (range, _) in &looks {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut pieces = Vec::new();
    for pair in cuts.windows(2) {
        let piece = pair[0]..pair[1];
        let look = looks
            .iter()
            .find(|(range, _)| range.start <= piece.start && piece.end <= range.end)
            .map(|(_, look)| *look);
        let inside = selected.start <= piece.start && piece.end <= selected.end;
        match (look, inside) {
            (Some(look), true) => pieces.push((
                piece,
                HighlightStyle {
                    background_color: shade.background_color,
                    ..look
                },
            )),
            (Some(look), false) => pieces.push((piece, look)),
            (None, true) => pieces.push((piece, shade)),
            (None, false) => {}
        }
    }
    pieces
}

impl RenderOnce for Selectable {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = cx.default_global::<Selected>();
        // A range that does not fit this text (two blocks with one key) is not drawn.
        let range = state.0.range_of(&self.key).filter(|range| {
            self.content.is_char_boundary(range.start) && self.content.is_char_boundary(range.end)
        });
        let dragging = state.0.is_dragging_in(&self.key);
        let looks = match range {
            Some(range) => shaded(
                self.highlights,
                range,
                HighlightStyle {
                    background_color: Some(rgb(self.palette.selection).into()),
                    ..HighlightStyle::default()
                },
            ),
            None => self.highlights,
        };
        let styled = StyledText::new(self.content.clone())
            .with_highlights(looks)
            .with_font_family_overrides(self.mono);
        let layout = styled.layout().clone();
        let text: AnyElement = if self.links.is_empty() {
            styled.into_any_element()
        } else {
            let (ranges, addresses): (Vec<_>, Vec<_>) = self.links.into_iter().unzip();
            InteractiveText::new(ElementId::from(self.key.clone()), styled)
                .on_click(ranges, move |index, _, cx: &mut App| {
                    if let Some(address) = addresses.get(index) {
                        cx.open_url(address);
                    }
                })
                .into_any_element()
        };
        let (key, content) = (self.key, self.content);
        let (pressed_key, pressed_content, pressed_layout) =
            (key.clone(), content.clone(), layout.clone());
        div()
            .relative()
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let at = offset_at(&pressed_layout, event.position, pressed_content.len());
                    cx.default_global::<Selected>().0.press(
                        &pressed_key,
                        &pressed_content,
                        at,
                        event.click_count,
                    );
                    window.refresh();
                },
            )
            .child(text)
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |_, _, window, _| {
                        if dragging {
                            follow_drag(&key, &layout, &content, window);
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}

fn offset_at(layout: &TextLayout, position: Point<Pixels>, length: usize) -> usize {
    let (Ok(at) | Err(at)) = layout.index_for_position(position);
    at.min(length)
}

/// While a selection is dragged the pointer is followed over the whole window, not only over the
/// block: past its edge the selection goes on, and the button released anywhere ends it.
fn follow_drag(
    key: &SharedString,
    layout: &TextLayout,
    content: &SharedString,
    window: &mut Window,
) {
    let (key, layout, content) = (key.clone(), layout.clone(), content.clone());
    follow(
        window,
        move |event, window, cx| {
            let at = offset_at(&layout, event.position, content.len());
            if cx.default_global::<Selected>().0.drag(&key, &content, at) {
                window.refresh();
            }
        },
        |window, cx| {
            cx.default_global::<Selected>().0.release();
            window.refresh();
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> HighlightStyle {
        HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..HighlightStyle::default()
        }
    }

    fn shade() -> HighlightStyle {
        HighlightStyle {
            background_color: Some(gpui::hsla(0.5, 0.5, 0.5, 1.0)),
            ..HighlightStyle::default()
        }
    }

    #[test]
    fn a_selection_over_plain_text_is_only_the_shade() {
        let pieces = shaded(Vec::new(), 2..5, shade());
        assert_eq!(pieces, vec![(2..5, shade())]);
    }

    #[test]
    fn a_look_the_selection_cuts_keeps_its_look_and_takes_the_shade_inside() {
        let pieces = shaded(vec![(0..6, bold())], 4..9, shade());
        assert_eq!(
            pieces,
            vec![
                (0..4, bold()),
                (
                    4..6,
                    HighlightStyle {
                        background_color: shade().background_color,
                        ..bold()
                    }
                ),
                (6..9, shade()),
            ]
        );
    }

    #[test]
    fn looks_outside_the_selection_are_untouched() {
        let pieces = shaded(vec![(0..2, bold()), (8..10, bold())], 3..6, shade());
        assert_eq!(
            pieces,
            vec![(0..2, bold()), (3..6, shade()), (8..10, bold())]
        );
    }
}
