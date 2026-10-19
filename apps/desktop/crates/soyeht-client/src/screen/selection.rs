//! Selecting text in a terminal: a drag selects cells, a double-click a word, a triple-click a
//! line, as in every terminal (and in SwiftTerm, which the Mac client uses).
//!
//! Places are lines counted from the oldest one kept, so a selection stays on its text while the
//! view scrolls and while output pushes lines into the history.

use super::lines::{Lines, Logical};

/// Characters other than letters and digits that belong to a word, so that a double-click takes a
/// whole path, address or option (the set gnome-terminal uses, without the comma).
const WORD_CHARACTERS: &str = "-./~:@%+=#?&_";

/// A cell of the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub line: usize,
    pub column: u16,
}

/// What a selection grows by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Cell,
    Word,
    Line,
}

impl Unit {
    /// What the `clicks`th click of a row of clicks selects: a drag from the first, a word from
    /// the second and a line from the third on.
    pub fn of_clicks(clicks: usize) -> Self {
        match clicks {
            0 | 1 => Self::Cell,
            2 => Self::Word,
            _ => Self::Line,
        }
    }
}

/// The cells from `start` to `end`, both included. A wide character's `end` is its second column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Point,
    pub end: Point,
}

impl Span {
    /// The columns the span covers in `line`, from the first to one past the last, in a terminal
    /// `columns` wide. A line in the middle is covered whole.
    pub fn columns_in(&self, line: usize, columns: u16) -> Option<(u16, u16)> {
        if line < self.start.line || line > self.end.line {
            return None;
        }
        let from = if line == self.start.line {
            self.start.column
        } else {
            0
        };
        let to = if line == self.end.line {
            self.end.column.saturating_add(1)
        } else {
            columns
        };
        let to = to.min(columns);
        (from < to).then_some((from, to))
    }
}

/// What the characters of a line are sorted into by a double-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Other,
}

fn class_of(character: Option<char>) -> Class {
    match character {
        Some(character) if character.is_whitespace() => Class::Space,
        Some(character) if character.is_alphanumeric() || WORD_CHARACTERS.contains(character) => {
            Class::Word
        }
        _ => Class::Other,
    }
}

/// The selection a person is making or has made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// What the press selected, which the selection never gets smaller than.
    anchor: Span,
    /// What the pointer is over now.
    head: Span,
    unit: Unit,
}

impl Selection {
    /// A press at `at`.
    pub(crate) fn begin(lines: &mut Lines, at: Point, unit: Unit) -> Self {
        let span = span_at(lines, at, unit);
        Self {
            anchor: span,
            head: span,
            unit,
        }
    }

    /// The pointer moved to `to` with the button down.
    pub(crate) fn extend(&mut self, lines: &mut Lines, to: Point) {
        self.head = span_at(lines, to, self.unit);
    }

    /// What is selected. A press with no drag selects nothing.
    pub fn span(&self) -> Option<Span> {
        if self.unit == Unit::Cell && self.anchor == self.head {
            return None;
        }
        Some(Span {
            start: self.anchor.start.min(self.head.start),
            end: self.anchor.end.max(self.head.end),
        })
    }

    /// The text selected: a row ends in a line break unless it wraps into the next, and the
    /// blanks at the end of a row are not text.
    pub(crate) fn text(&self, lines: &mut Lines) -> Option<String> {
        let span = self.span()?;
        Some(text_of(lines, span, 0))
    }

    /// The part of the text that is on the live screen from `first_live` on, which is all that
    /// output can change: the lines above it are history and stay as they are.
    pub(crate) fn live_text(&self, lines: &mut Lines, first_live: usize) -> String {
        self.span()
            .map(|span| text_of(lines, span, first_live))
            .unwrap_or_default()
    }
}

fn span_at(lines: &mut Lines, at: Point, unit: Unit) -> Span {
    let last_line = lines.end().saturating_sub(1);
    let at = Point {
        line: at.line.min(last_line),
        column: at.column.min(lines.columns().saturating_sub(1)),
    };
    let logical = Logical::around(lines, at.line);
    let Some(index) = logical.index_of(at.line, at.column) else {
        return Span { start: at, end: at };
    };
    let glyphs = &logical.glyphs;
    let point = |at: &super::lines::Glyph| Point {
        line: at.line,
        column: at.column,
    };
    match unit {
        Unit::Cell => Span {
            start: point(&glyphs[index]),
            end: Point {
                line: glyphs[index].line,
                column: glyphs[index].last_column(),
            },
        },
        Unit::Word => {
            let class = class_of(glyphs[index].character());
            let same = |glyph: &super::lines::Glyph| class_of(glyph.character()) == class;
            let mut from = index;
            while from > 0 && same(&glyphs[from - 1]) {
                from -= 1;
            }
            let mut to = index;
            while to + 1 < glyphs.len() && same(&glyphs[to + 1]) {
                to += 1;
            }
            Span {
                start: point(&glyphs[from]),
                end: Point {
                    line: glyphs[to].line,
                    column: glyphs[to].last_column(),
                },
            }
        }
        Unit::Line => Span {
            start: Point {
                line: logical.first,
                column: 0,
            },
            end: Point {
                line: logical.last,
                column: lines.columns().saturating_sub(1),
            },
        },
    }
}

/// The text of `span`, leaving out the lines before `skip_before`.
fn text_of(lines: &mut Lines, span: Span, skip_before: usize) -> String {
    let last_column = lines.columns().saturating_sub(1);
    let mut text = String::new();
    for line in span.start.line.max(skip_before)..=span.end.line.min(lines.end().saturating_sub(1))
    {
        let from = if line == span.start.line {
            span.start.column
        } else {
            0
        };
        let to = if line == span.end.line {
            span.end.column
        } else {
            last_column
        };
        let row = lines.text(line, from, to);
        // A row that wraps is full: its last blanks are part of the line.
        if line != span.end.line && lines.wrapped(line) {
            text.push_str(&row);
        } else {
            text.push_str(row.trim_end());
            if line != span.end.line {
                text.push('\n');
            }
        }
    }
    text
}

/// Where a pointer is over a terminal's cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Located {
    pub row: u16,
    pub column: u16,
    /// The pointer is above the first row (a drag there scrolls toward older lines).
    pub above: bool,
    /// The pointer is below the last row.
    pub below: bool,
}

/// The cell of a terminal drawn from the top-left corner `origin` that the pointer `at` is over.
/// A pointer outside the grid is placed on the nearest cell; `above` and `below` say it was out.
pub fn locate(at: (f32, f32), origin: (f32, f32), cell: (f32, f32), grid: (u16, u16)) -> Located {
    let (rows, columns) = grid;
    let index = |offset: f32, size: f32, count: u16| -> (u16, bool, bool) {
        let place = (offset / size.max(f32::EPSILON)).floor();
        let last = f32::from(count.saturating_sub(1));
        if place < 0.0 {
            (0, true, false)
        } else if place > last {
            (count.saturating_sub(1), false, true)
        } else {
            (place as u16, false, false)
        }
    };
    let (row, above, below) = index(at.1 - origin.1, cell.1, rows);
    let (column, _, _) = index(at.0 - origin.0, cell.0, columns);
    Located {
        row,
        column,
        above,
        below,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (f32, f32) = (8.0, 16.0);

    #[test]
    fn a_pointer_over_the_grid_is_on_its_cell() {
        let found = locate((100.0, 50.0), (10.0, 20.0), CELL, (24, 80));
        assert_eq!((found.row, found.column), (1, 11));
        assert!(!found.above && !found.below);
        let corner = locate((10.0, 20.0), (10.0, 20.0), CELL, (24, 80));
        assert_eq!((corner.row, corner.column), (0, 0));
    }

    #[test]
    fn a_pointer_outside_the_grid_is_on_the_nearest_cell_and_says_which_side() {
        let above = locate((5.0, 0.0), (10.0, 20.0), CELL, (24, 80));
        assert_eq!((above.row, above.column), (0, 0));
        assert!(above.above && !above.below);
        let below = locate((9999.0, 9999.0), (10.0, 20.0), CELL, (24, 80));
        assert_eq!((below.row, below.column), (23, 79));
        assert!(below.below && !below.above);
        let edge = (10.0 + 8.0 * 80.0 - 0.5, 20.0 + 16.0 * 24.0 - 0.5);
        let inside = locate(edge, (10.0, 20.0), CELL, (24, 80));
        assert_eq!((inside.row, inside.column), (23, 79));
        assert!(!inside.below);
    }

    #[test]
    fn clicks_choose_the_unit() {
        assert_eq!(Unit::of_clicks(1), Unit::Cell);
        assert_eq!(Unit::of_clicks(2), Unit::Word);
        assert_eq!(Unit::of_clicks(3), Unit::Line);
        assert_eq!(Unit::of_clicks(7), Unit::Line);
    }

    #[test]
    fn a_span_covers_whole_rows_in_the_middle() {
        let span = Span {
            start: Point { line: 2, column: 5 },
            end: Point { line: 4, column: 1 },
        };
        assert_eq!(span.columns_in(1, 10), None);
        assert_eq!(span.columns_in(2, 10), Some((5, 10)));
        assert_eq!(span.columns_in(3, 10), Some((0, 10)));
        assert_eq!(span.columns_in(4, 10), Some((0, 2)));
        assert_eq!(span.columns_in(5, 10), None);
    }
}
