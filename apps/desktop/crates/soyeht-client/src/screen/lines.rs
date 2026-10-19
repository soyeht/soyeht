//! Reading any line of a terminal by its number, the history above the view included.
//!
//! The parser can only show one window of lines at a time (the view and how far it is scrolled
//! back). A selection and a link are in lines counted from the oldest one kept, so that they stay
//! where they are while the view scrolls and while new output pushes lines into the history.

/// The most rows a line that wraps is followed over, so that output with no line break cannot
/// make a double-click read the whole history.
const MAX_WRAPPED_ROWS: usize = 256;

/// The lines of one terminal. The view is put back where it was when this is dropped.
pub(crate) struct Lines<'a> {
    parser: &'a mut vt100::Parser,
    /// How far the view was scrolled back when this was made.
    saved: usize,
    /// The lines above the live screen.
    history: usize,
    rows: u16,
    cols: u16,
    /// The line shown in the first row of the window.
    top: usize,
}

impl<'a> Lines<'a> {
    pub fn new(parser: &'a mut vt100::Parser) -> Self {
        let screen = parser.screen_mut();
        let saved = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let history = screen.scrollback();
        screen.set_scrollback(saved);
        let (rows, cols) = screen.size();
        Self {
            parser,
            saved,
            history,
            rows,
            cols,
            top: history.saturating_sub(saved),
        }
    }

    /// The number of lines above the live screen.
    pub fn history(&self) -> usize {
        self.history
    }

    pub fn columns(&self) -> u16 {
        self.cols
    }

    /// One past the last line there is.
    pub fn end(&self) -> usize {
        self.history + usize::from(self.rows)
    }

    /// Moves the window so that `line` is in it, and says which row it is.
    fn show(&mut self, line: usize) -> Option<u16> {
        if line >= self.end() {
            return None;
        }
        let rows = usize::from(self.rows);
        if line < self.top || line >= self.top + rows {
            let offset = self.history.saturating_sub(line);
            self.parser.screen_mut().set_scrollback(offset);
            self.top = self.history - offset.min(self.history);
        }
        // The line is in the window now, whose rows number at most a `u16`.
        Some((line - self.top) as u16)
    }

    pub fn cell(&mut self, line: usize, column: u16) -> Option<&vt100::Cell> {
        let row = self.show(line)?;
        self.parser.screen().cell(row, column)
    }

    /// Whether the text of `line` goes on in the next one.
    pub fn wrapped(&mut self, line: usize) -> bool {
        self.show(line)
            .is_some_and(|row| self.parser.screen().row_wrapped(row))
    }

    /// The text of the cells of `line` from `from` to `to`, both included. A cell with nothing
    /// in it is a space, and the second half of a wide character adds nothing.
    pub fn text(&mut self, line: usize, from: u16, to: u16) -> String {
        let mut text = String::new();
        for column in from..=to.min(self.cols.saturating_sub(1)) {
            match self.cell(line, column) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if cell.has_contents() => text.push_str(cell.contents()),
                _ => text.push(' '),
            }
        }
        text
    }
}

impl Drop for Lines<'_> {
    fn drop(&mut self) {
        self.parser.screen_mut().set_scrollback(self.saved);
    }
}

/// One character of a logical line: where it starts and how many columns it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Glyph {
    pub line: usize,
    pub column: u16,
    pub columns: u16,
    pub text: String,
}

impl Glyph {
    /// The last column the glyph takes.
    pub fn last_column(&self) -> u16 {
        self.column + self.columns - 1
    }

    /// The character, when the cell holds one (a cell of combining marks holds several).
    pub fn character(&self) -> Option<char> {
        let mut characters = self.text.chars();
        characters.next().filter(|_| characters.next().is_none())
    }
}

/// A line of text as the program wrote it: the rows that wrap into one another.
pub(crate) struct Logical {
    pub glyphs: Vec<Glyph>,
    /// The first and last row of the line.
    pub first: usize,
    pub last: usize,
}

impl Logical {
    pub fn around(lines: &mut Lines, line: usize) -> Self {
        let line = line.min(lines.end().saturating_sub(1));
        let mut first = line;
        let mut walked = 0;
        while first > 0 && walked < MAX_WRAPPED_ROWS && lines.wrapped(first - 1) {
            first -= 1;
            walked += 1;
        }
        let mut last = line;
        walked = 0;
        while last + 1 < lines.end() && walked < MAX_WRAPPED_ROWS && lines.wrapped(last) {
            last += 1;
            walked += 1;
        }
        let mut glyphs = Vec::new();
        for row in first..=last {
            for column in 0..lines.columns() {
                let Some(cell) = lines.cell(row, column) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                glyphs.push(Glyph {
                    line: row,
                    column,
                    columns: if cell.is_wide() { 2 } else { 1 },
                    text: if cell.has_contents() {
                        cell.contents().to_owned()
                    } else {
                        " ".to_owned()
                    },
                });
            }
        }
        Self {
            glyphs,
            first,
            last,
        }
    }

    /// The glyph that takes the cell at `line` and `column`.
    pub fn index_of(&self, line: usize, column: u16) -> Option<usize> {
        let after = self
            .glyphs
            .partition_point(|glyph| (glyph.line, glyph.column) <= (line, column));
        let index = after.checked_sub(1)?;
        let glyph = &self.glyphs[index];
        (glyph.line == line && column <= glyph.last_column()).then_some(index)
    }
}
