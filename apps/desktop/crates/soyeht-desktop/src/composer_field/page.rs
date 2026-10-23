//! The page of a screenplay: the text of a field laid out one line of the file at a time, each
//! line aligned, set in, bold or in the secondary ink as its element says
//! (`soyeht_client::writing::screenplay`), with the markers of a line left out of what is drawn.
//! The text stays one string; `Laid` answers, for the whole page, the questions the field asks
//! of a layout (where an offset is, which offset a point is) in the offsets of that string.

use super::{composing_style, selected_style};
use crate::theme::{Palette, desktop};
use gpui::{
    Div, FontWeight, ParentElement, Pixels, Point, Styled, StyledText, TextLayout, div,
    prelude::FluentBuilder, px, relative, rgb,
};
use soyeht_client::{
    text_input::TextInput,
    writing::screenplay::{Alignment, LineStyle},
};
use std::ops::Range;

/// What a field asks of the text it drew: a point of an offset, an offset of a point, how tall a
/// line is.
pub(super) enum Laid {
    /// The text drawn as one block.
    Whole(TextLayout),
    /// The page of a screenplay, line by line.
    Page(Vec<Row>),
}

/// A line of the file as it was drawn.
pub(super) struct Row {
    /// Where the line starts in the text, and how long it is.
    start: usize,
    len: usize,
    /// The markers that were left out of what is drawn, as ranges of the line.
    hidden: Vec<Range<usize>>,
    /// The bytes that were drawn.
    shown: usize,
    layout: TextLayout,
}

impl Row {
    /// Where a byte of the line is among the bytes that were drawn: the markers take no room.
    fn drawn(&self, offset: usize) -> usize {
        drawn_at(&self.hidden, offset).min(self.shown)
    }

    /// The byte of the line that a byte that was drawn stands for.
    fn written(&self, drawn: usize) -> usize {
        let mut offset = drawn.min(self.shown);
        for marker in &self.hidden {
            if offset >= marker.start {
                offset += marker.len();
            } else {
                break;
            }
        }
        offset.min(self.len)
    }
}

/// Where byte `offset` of a line is among the bytes that are drawn when `hidden` are not.
fn drawn_at(hidden: &[Range<usize>], offset: usize) -> usize {
    let skipped: usize = hidden
        .iter()
        .map(|marker| offset.min(marker.end).saturating_sub(marker.start))
        .sum();
    offset - skipped
}

impl Laid {
    pub(super) fn line_height(&self) -> Pixels {
        match self {
            Self::Whole(layout) => layout.line_height(),
            Self::Page(rows) => rows.first().map_or_else(
                || px(desktop::SCREENPLAY_LINE_HEIGHT),
                |row| row.layout.line_height(),
            ),
        }
    }

    pub(super) fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        match self {
            Self::Whole(layout) => layout.position_for_index(index),
            Self::Page(rows) => {
                let row = rows
                    .iter()
                    .rev()
                    .find(|row| row.start <= index)
                    .or(rows.first())?;
                row.layout
                    .position_for_index(row.drawn(index.saturating_sub(row.start)))
            }
        }
    }

    pub(super) fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        match self {
            Self::Whole(layout) => layout.index_for_position(position),
            Self::Page(rows) => {
                let Some(row) = rows
                    .iter()
                    .find(|row| row.layout.bounds().bottom() >= position.y)
                    .or(rows.last())
                else {
                    return Err(0);
                };
                let found = row.layout.index_for_position(position);
                let (Ok(drawn) | Err(drawn)) = found;
                let at = row.start + row.written(drawn);
                if found.is_ok() { Ok(at) } else { Err(at) }
            }
        }
    }
}

/// The lines of `draft` as a page: the element to put in the field's place, and how it was laid
/// out once it is painted. `styles` has the style of each line of the text; a line it does not
/// reach is plain.
pub(super) fn page(draft: &TextInput, styles: &[LineStyle], palette: &Palette) -> (Div, Laid) {
    let text = draft.text();
    let selection = draft.has_selection().then(|| draft.selection());
    let marked = draft.marked_bytes();
    let plain = LineStyle::default();
    let mut rows = Vec::new();
    let mut elements = Vec::new();
    let mut start = 0;
    for (index, line) in text.split('\n').enumerate() {
        let style = styles.get(index).unwrap_or(&plain);
        let end = start + line.len();
        let (shown, drawn) = drawn_text(line, &style.hidden);
        // A line break inside the selection is drawn as a space after the line, and so is the
        // nothing a line holds, which keeps its height.
        let breaks_in = selection
            .as_ref()
            .is_some_and(|selected| selected.start <= end && selected.end > end);
        let mut shown_text = shown.clone();
        if shown.is_empty() || breaks_in {
            shown_text.push(' ');
        }
        let mut highlights = Vec::new();
        if let Some(marked) = marked
            .as_ref()
            .and_then(|marked| within(marked, start, end))
        {
            highlights.push((span(&style.hidden, &marked), composing_style(palette)));
        } else if let Some(selected) = selection.as_ref().and_then(|s| within(s, start, end)) {
            let mut covered = span(&style.hidden, &selected);
            if breaks_in {
                covered.end = shown_text.len();
            }
            highlights.push((covered, selected_style(palette)));
        }
        highlights.retain(|(range, _)| range.start < range.end);
        let styled = StyledText::new(shown_text).with_highlights(highlights);
        let layout = styled.layout().clone();
        elements.push(row_of(styled, style, palette));
        rows.push(Row {
            start,
            len: line.len(),
            hidden: style.hidden.clone(),
            shown: drawn,
            layout,
        });
        start = end + 1;
    }
    (div().children(elements), Laid::Page(rows))
}

/// What is drawn of a line when its markers are not, and how many bytes that is.
fn drawn_text(line: &str, hidden: &[Range<usize>]) -> (String, usize) {
    let mut shown = String::with_capacity(line.len());
    let mut from = 0;
    for marker in hidden.iter().filter(|marker| marker.end <= line.len()) {
        shown.push_str(&line[from..marker.start]);
        from = marker.end;
    }
    shown.push_str(&line[from..]);
    let drawn = shown.len();
    (shown, drawn)
}

/// The part of `range` that lies in the line `start..end`, as a range of the line; a range that
/// reaches the end of the line reaches it, and a line break after it is no part of it.
fn within(range: &Range<usize>, start: usize, end: usize) -> Option<Range<usize>> {
    (range.start <= end && range.end >= start)
        .then(|| range.start.max(start) - start..range.end.min(end) - start)
}

/// A range of a line as a range of what is drawn of it.
fn span(hidden: &[Range<usize>], range: &Range<usize>) -> Range<usize> {
    drawn_at(hidden, range.start)..drawn_at(hidden, range.end)
}

/// A line on the page: set in from both margins, aligned, bold or in the secondary ink.
fn row_of(text: StyledText, style: &LineStyle, palette: &Palette) -> Div {
    let line = div().max_w_full().child(text);
    let aligned = match style.alignment {
        Alignment::Center => div().flex().justify_center().child(line),
        Alignment::Right => div().flex().justify_end().child(line),
        Alignment::Natural | Alignment::Left => div().child(line),
    };
    aligned
        .w_full()
        .pb(px(desktop::SCREENPLAY_PARAGRAPH_SPACING))
        .when(style.inset, |row| {
            row.px(relative(desktop::SCREENPLAY_DIALOGUE_SHARE))
        })
        .when(style.bold, |row| row.font_weight(FontWeight::BOLD))
        .when(style.secondary, |row| {
            row.text_color(rgb(palette.secondary))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_markers_take_no_room_and_the_bytes_that_are_drawn_find_their_place() {
        let hidden = [0..1, 4..5];
        // "@ANA:" is drawn as "ANA".
        let (shown, drawn) = drawn_text("@ANA:", &hidden);
        assert_eq!((shown.as_str(), drawn), ("ANA", 3));
        let offsets: Vec<usize> = (0..=5).map(|at| drawn_at(&hidden, at)).collect();
        assert_eq!(offsets, [0, 0, 1, 2, 3, 3]);
    }

    #[test]
    fn a_range_in_a_line_is_the_part_of_it_the_line_holds() {
        assert_eq!(within(&(2..9), 4, 8), Some(0..4));
        assert_eq!(within(&(5..6), 4, 8), Some(1..2));
        assert_eq!(within(&(9..12), 4, 8), None);
        // A selection that ends where the line starts holds nothing of it but its start.
        assert_eq!(within(&(0..4), 4, 8), Some(0..0));
    }

    #[test]
    fn a_range_of_a_line_is_a_range_of_what_is_drawn() {
        let hidden = [0..1, 4..5];
        assert_eq!(span(&hidden, &(1..4)), 0..3);
        assert_eq!(span(&hidden, &(0..5)), 0..3);
    }
}
