//! The screen of one terminal pane. The engine streams raw terminal bytes, exactly as it does
//! to the Mac client; this type turns them into styled rows a view can paint.

/// Lines of history kept per terminal on the client.
const SCROLLBACK_LINES: usize = 10_000;

/// A cell color before a theme gives it a value. `DefaultFg` and `DefaultBg` stay distinct so
/// reverse video can swap them without the view guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    DefaultFg,
    DefaultBg,
    /// One of the 256 indexed colors; the first 16 come from the theme's ANSI slots.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Consecutive cells that share one style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub column: u16,
    /// Width in terminal columns (wide characters count twice).
    pub columns: u16,
    pub text: String,
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: u16,
    pub column: u16,
}

pub struct TerminalScreen {
    parser: vt100::Parser,
    /// The part of a line the wheel has moved that is too small to scroll yet.
    scroll_remainder: f32,
}

impl TerminalScreen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows.max(1), cols.max(1), SCROLLBACK_LINES),
            scroll_remainder: 0.0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Forgets everything shown, as a replay that starts from scratch requires.
    pub fn reset(&mut self) {
        let (rows, cols) = self.size();
        self.parser = vt100::Parser::new(rows, cols, SCROLLBACK_LINES);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if (rows, cols) != self.size() {
            self.parser.screen_mut().set_size(rows, cols);
        }
    }

    /// `(rows, columns)`.
    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    /// Scrolls the view into history. Positive moves up toward older lines.
    pub fn scroll(&mut self, lines: i32) {
        let screen = self.parser.screen_mut();
        let current = i64::try_from(screen.scrollback()).unwrap_or(0);
        let target = (current + i64::from(lines)).max(0);
        screen.set_scrollback(usize::try_from(target).unwrap_or(0));
    }

    /// Scrolls by a fractional number of lines, as a trackpad reports it. What does not make a
    /// whole line is kept for the next call, so slow scrolling still moves. Returns whether the
    /// view moved.
    pub fn scroll_by(&mut self, lines: f32) -> bool {
        let total = self.scroll_remainder + lines;
        let whole = total.trunc();
        self.scroll_remainder = total - whole;
        let before = self.parser.screen().scrollback();
        self.scroll(whole as i32);
        self.parser.screen().scrollback() != before
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_remainder = 0.0;
        self.parser.screen_mut().set_scrollback(0);
    }

    pub fn scrolled_back(&self) -> bool {
        self.parser.screen().scrollback() > 0
    }

    /// The cursor, unless the program hid it or the view shows history.
    pub fn cursor(&self) -> Option<Cursor> {
        let screen = self.parser.screen();
        if screen.hide_cursor() || screen.scrollback() > 0 {
            return None;
        }
        let (row, column) = screen.cursor_position();
        Some(Cursor { row, column })
    }

    /// Arrow keys send `ESC O` instead of `ESC [` while a program asks for it.
    pub fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// The character in a cell, or a space when it is empty.
    pub fn text_at(&self, row: u16, column: u16) -> String {
        self.parser
            .screen()
            .cell(row, column)
            .filter(|cell| cell.has_contents())
            .map_or_else(|| " ".to_owned(), |cell| cell.contents().to_owned())
    }

    pub fn row_runs(&self, row: u16) -> Vec<Run> {
        let screen = self.parser.screen();
        let (_, cols) = screen.size();
        let mut runs: Vec<Run> = Vec::new();
        // A wide character is a run of its own, so a painter that places every character in a
        // one-column slot cannot shift what follows it.
        let mut after_wide = false;
        for column in 0..cols {
            let Some(cell) = screen.cell(row, column) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let (mut fg, mut bg) = (
                color(cell.fgcolor(), CellColor::DefaultFg),
                color(cell.bgcolor(), CellColor::DefaultBg),
            );
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            let width = if cell.is_wide() { 2 } else { 1 };
            let text = if cell.has_contents() {
                cell.contents()
            } else {
                " "
            };
            let style = (
                fg,
                bg,
                cell.bold(),
                cell.italic(),
                cell.underline(),
                cell.dim(),
            );
            let joins_previous = width == 1 && !after_wide;
            after_wide = width == 2;
            if joins_previous
                && let Some(last) = runs.last_mut()
                && (
                    last.fg,
                    last.bg,
                    last.bold,
                    last.italic,
                    last.underline,
                    last.dim,
                ) == style
            {
                last.text.push_str(text);
                last.columns += width;
                continue;
            }
            runs.push(Run {
                column,
                columns: width,
                text: text.to_owned(),
                fg,
                bg,
                bold: style.2,
                italic: style.3,
                underline: style.4,
                dim: style.5,
            });
        }
        runs
    }
}

fn color(value: vt100::Color, default: CellColor) -> CellColor {
    match value {
        vt100::Color::Default => default,
        vt100::Color::Idx(index) => CellColor::Indexed(index),
        vt100::Color::Rgb(r, g, b) => CellColor::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(rows: u16, cols: u16, input: &str) -> TerminalScreen {
        let mut screen = TerminalScreen::new(rows, cols);
        screen.feed(input.as_bytes());
        screen
    }

    #[test]
    fn slow_scrolling_accumulates_fractions_until_a_line_moves() {
        let history: String = (0..30).map(|n| format!("line {n}\r\n")).collect();
        let mut screen = screen(5, 20, &history);
        for _ in 0..3 {
            assert!(!screen.scroll_by(0.3));
            assert!(!screen.scrolled_back());
        }
        assert!(screen.scroll_by(0.3));
        assert!(screen.scrolled_back());
        // 1.2 lines made one; the other 0.2 stays. Scrolling back down uses it up first.
        assert!(!screen.scroll_by(-0.3));
        assert!(screen.scroll_by(-1.0));
        assert!(!screen.scrolled_back());
    }

    #[test]
    fn plain_text_is_one_default_styled_run() {
        let screen = screen(2, 10, "hello");
        let runs = screen.row_runs(0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "hello     ");
        assert_eq!(
            (runs[0].fg, runs[0].bg),
            (CellColor::DefaultFg, CellColor::DefaultBg)
        );
        assert_eq!(screen.cursor(), Some(Cursor { row: 0, column: 5 }));
    }

    #[test]
    fn color_changes_split_runs_at_the_right_columns() {
        let screen = screen(1, 8, "ab\x1b[31mcd\x1b[0mef");
        let runs = screen.row_runs(0);
        let texts: Vec<_> = runs
            .iter()
            .map(|run| (run.column, run.text.as_str()))
            .collect();
        assert_eq!(texts, [(0, "ab"), (2, "cd"), (4, "ef  ")]);
        assert_eq!(runs[1].fg, CellColor::Indexed(1));
    }

    #[test]
    fn inverse_video_swaps_the_default_roles() {
        let screen = screen(1, 4, "\x1b[7mx");
        let first = &screen.row_runs(0)[0];
        assert_eq!(
            (first.fg, first.bg),
            (CellColor::DefaultBg, CellColor::DefaultFg)
        );
    }

    #[test]
    fn wide_characters_take_two_columns() {
        let screen = screen(1, 6, "a\u{4f60}b");
        let runs = screen.row_runs(0);
        let columns: u16 = runs.iter().map(|run| run.columns).sum();
        assert_eq!(columns, 6);
        let texts: Vec<_> = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(texts, ["a", "\u{4f60}", "b  "]);
        assert_eq!(screen.text_at(0, 1), "\u{4f60}");
        assert_eq!(screen.text_at(0, 5), " ");
    }

    #[test]
    fn reset_clears_the_screen_and_keeps_the_size() {
        let mut screen = screen(3, 7, "abc\r\ndef");
        screen.reset();
        assert_eq!(screen.size(), (3, 7));
        assert!(screen.contents().trim().is_empty());
    }

    #[test]
    fn resize_changes_the_grid() {
        let mut screen = screen(3, 7, "abc");
        screen.resize(5, 20);
        assert_eq!(screen.size(), (5, 20));
    }

    #[test]
    fn scrolling_into_history_hides_the_cursor_and_returns_to_the_bottom() {
        let mut text = String::new();
        for line in 0..12 {
            text.push_str(&format!("line {line}\r\n"));
        }
        let mut screen = screen(4, 10, &text);
        assert!(screen.cursor().is_some());
        screen.scroll(3);
        assert!(screen.scrolled_back());
        assert!(screen.cursor().is_none());
        screen.scroll(-100);
        assert!(!screen.scrolled_back());
    }

    #[test]
    fn program_modes_are_reported() {
        let screen = screen(2, 10, "\x1b[?1h\x1b[?2004h");
        assert!(screen.application_cursor());
        assert!(screen.bracketed_paste());
    }
}
