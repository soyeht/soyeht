//! Fitting a screen to a new size the way SwiftTerm does for the Mac client: the lines above
//! the one the cursor is on keep their text and are cut again at the new width (they join again
//! when it grows), the lines of the history come back when there are more rows, and the line
//! the cursor is on is left as it is, cut where it no longer fits. A shell draws that line
//! again when it learns the new size, and counts on the terminal not having moved its text.
//!
//! The parser cuts rows and leaves the cursor where it was, so the screen is built again from its
//! lines. One thing is kept that SwiftTerm does not: what a narrower screen cut from the
//! cursor's line. A pane that is made narrower and wide again before the shell has heard of
//! it (it is told the last size only) would otherwise keep a prompt cut in two with the cursor
//! in the middle of it, and nothing would draw it again.

use super::{Lines, SCROLLBACK_LINES};
use vt100::{Cell, Color};

/// The attributes that make a cell look the way it does, besides its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Style {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Style {
    const PLAIN: Self = Self {
        fg: Color::Default,
        bg: Color::Default,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        inverse: false,
    };

    fn of(cell: &Cell) -> Self {
        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    /// The sequence that sets exactly these attributes.
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[0");
        for (on, code) in [
            (self.bold, 1),
            (self.dim, 2),
            (self.italic, 3),
            (self.underline, 4),
            (self.inverse, 7),
        ] {
            if on {
                out.extend_from_slice(format!(";{code}").as_bytes());
            }
        }
        write_color(out, self.fg, 30);
        write_color(out, self.bg, 40);
        out.push(b'm');
    }
}

/// `base` is 30 for a foreground and 40 for a background.
fn write_color(out: &mut Vec<u8>, color: Color, base: u8) {
    let code = match color {
        Color::Default => return,
        Color::Idx(index) if index < 8 => format!(";{}", base + index),
        Color::Idx(index) if index < 16 => format!(";{}", base + 60 + (index - 8)),
        Color::Idx(index) => format!(";{};5;{index}", base + 8),
        Color::Rgb(red, green, blue) => format!(";{};2;{red};{green};{blue}", base + 8),
    };
    out.extend_from_slice(code.as_bytes());
}

/// A cell that shows nothing and can be left out at the end of a line.
fn is_blank(cell: &Cell) -> bool {
    !cell.has_contents() && !cell.is_wide_continuation() && Style::of(cell) == Style::PLAIN
}

/// Where the cursor of the old screen goes in the new one.
struct Place {
    /// Rows from the first one emitted.
    row: usize,
    column: u16,
}

/// What a narrower screen cut from the line the cursor is on, whole, while nothing has been
/// written to the screen since.
pub(super) struct Cut {
    rows: Vec<Vec<Cell>>,
    /// The column the cursor was in, before it was moved in to the screen's last one.
    cursor_column: u16,
}

impl Cut {
    /// Whether `rows`, the cursor's line as it is now, is what this cut left.
    fn left(&self, rows: &[Vec<Cell>]) -> bool {
        self.rows.len() == rows.len()
            && self
                .rows
                .iter()
                .zip(rows)
                .all(|(whole, shown)| whole.iter().zip(shown).all(|(a, b)| a == b))
    }
}

/// The state of writing the lines out at the new width.
struct Writer {
    out: Vec<u8>,
    columns: u16,
    /// The rows written so far, the line being written not counted.
    rows: usize,
    style: Style,
}

impl Writer {
    /// Writes the cells of one logical line, from the row `first` to the row `last`, cut again
    /// at the new width.
    fn line(&mut self, lines: &mut Lines, (first, last): (usize, usize)) {
        let end = self.text_end(lines, first, last);
        let mut column = 0u16;
        let mut row = 0usize;
        for line in first..=last {
            for old in 0..lines.columns() {
                let Some(cell) = lines.cell(line, old) else {
                    continue;
                };
                if cell.is_wide_continuation() || (line, old) >= end {
                    continue;
                }
                let width: u16 = if cell.is_wide() { 2 } else { 1 };
                if column + width > self.columns {
                    row += 1;
                    column = 0;
                }
                self.cell(cell);
                column += width;
            }
        }
        self.finish_line();
        self.rows += row + 1;
    }

    /// Writes the rows of the line the cursor is on one by one, each cut at the new width, and
    /// says where the cursor is.
    fn cursor_line(
        &mut self,
        rows: &[Vec<Cell>],
        (cursor_row, cursor_column): (usize, u16),
    ) -> Place {
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                self.out.extend_from_slice(b"\r\n");
            }
            let end = row
                .iter()
                .rposition(|cell| !is_blank(cell))
                .map_or(0, |last| last + 1);
            let mut column = 0u16;
            for cell in &row[..end] {
                if cell.is_wide_continuation() {
                    continue;
                }
                let width: u16 = if cell.is_wide() { 2 } else { 1 };
                if column + width > self.columns {
                    break;
                }
                self.cell(cell);
                column += width;
            }
            self.finish_line();
        }
        let place = Place {
            row: self.rows + cursor_row,
            column: cursor_column.min(self.columns - 1),
        };
        self.rows += rows.len();
        place
    }

    fn finish_line(&mut self) {
        if self.style != Style::PLAIN {
            Style::PLAIN.write(&mut self.out);
            self.style = Style::PLAIN;
        }
    }

    /// The first cell, as a row and a column, that is after the last one worth writing.
    fn text_end(&self, lines: &mut Lines, first: usize, last: usize) -> (usize, u16) {
        for line in (first..=last).rev() {
            for column in (0..lines.columns()).rev() {
                if lines.cell(line, column).is_some_and(|cell| !is_blank(cell)) {
                    return (line, column + 1);
                }
            }
        }
        (first, 0)
    }

    fn cell(&mut self, cell: &Cell) {
        let style = Style::of(cell);
        if style != self.style {
            style.write(&mut self.out);
            self.style = style;
        }
        if cell.has_contents() {
            self.out.extend_from_slice(cell.contents().as_bytes());
        } else {
            self.out.push(b' ');
        }
    }
}

/// The rows `first` to `last`, with every cell in them.
fn rows_of(lines: &mut Lines, first: usize, last: usize) -> Vec<Vec<Cell>> {
    (first..=last)
        .map(|line| {
            (0..lines.columns())
                .filter_map(|column| lines.cell(line, column).cloned())
                .collect()
        })
        .collect()
}

/// The screen of `parser`, a normal one, at `rows` by `cols` with its lines cut again to fit,
/// and what a narrower screen cut from the cursor's line, for a later resize. `restore` is what
/// the last resize cut, when nothing has been written since.
///
/// The new parser is only a carrier of the new screen: the caller moves the screen into the
/// parser that goes on receiving output, which keeps what it is in the middle of (an escape
/// sequence, a character split between two chunks). Of what the screen itself holds, the lines,
/// the cursor, the input modes and whether the cursor shows are carried, and the pen is for the
/// caller. Not carried, as in a cleared screen: autowrap off, insert mode, a charset, the scroll
/// region and the saved cursor; SwiftTerm also resets the region on a resize.
pub(super) fn rebuilt(
    parser: &mut vt100::Parser,
    (rows, cols): (u16, u16),
    restore: Option<Cut>,
) -> (vt100::Parser, Option<Cut>) {
    let screen = parser.screen();
    let (cursor_row, mut cursor_column) = screen.cursor_position();
    let input_modes = screen.input_mode_formatted();
    let cursor_hidden = screen.hide_cursor();
    let mut lines = Lines::new(parser);
    let cursor_line = lines.history() + usize::from(cursor_row);
    let (chain_first, chain_last) = chain_around(&mut lines, cursor_line);
    let last = last_line(&mut lines, chain_last);

    let mut chain = rows_of(&mut lines, chain_first, chain_last);
    if let Some(cut) = restore.filter(|cut| cut.left(&chain)) {
        chain = cut.rows;
        cursor_column = cut.cursor_column;
    }
    let mut writer = Writer {
        out: Vec::new(),
        columns: cols,
        rows: 0,
        style: Style::PLAIN,
    };
    let mut cursor = None;
    let mut line = 0;
    while line <= last {
        if line > 0 {
            writer.out.extend_from_slice(b"\r\n");
        }
        if line == chain_first {
            let at = (cursor_line - chain_first, cursor_column);
            cursor = Some(writer.cursor_line(&chain, at));
            line = chain_last + 1;
            continue;
        }
        let first = line;
        while line + 1 < lines.end() && lines.wrapped(line) {
            line += 1;
        }
        writer.line(&mut lines, (first, line));
        line += 1;
    }
    drop(lines);

    let cut = chain
        .iter()
        .any(|row| {
            row.iter()
                .skip(usize::from(cols))
                .any(|cell| !is_blank(cell))
        })
        .then_some(Cut {
            rows: chain,
            cursor_column,
        });
    let mut next = vt100::Parser::new(rows, cols, SCROLLBACK_LINES);
    if let Some(place) = cursor {
        // Lines below the cursor's can wrap into more rows than fit, which scrolls the cursor's
        // line out of the screen: the cursor is then on the first row.
        let top = writer.rows.saturating_sub(usize::from(rows));
        let row = place.row.saturating_sub(top);
        writer
            .out
            .extend_from_slice(format!("\x1b[{};{}H", row + 1, place.column + 1).as_bytes());
    }
    next.process(&writer.out);
    next.process(&input_modes);
    if cursor_hidden {
        next.process(b"\x1b[?25l");
    }
    (next, cut)
}

/// The first and the last row of the logical line `line` is in.
fn chain_around(lines: &mut Lines, line: usize) -> (usize, usize) {
    let mut first = line;
    while first > 0 && lines.wrapped(first - 1) {
        first -= 1;
    }
    let mut last = line;
    while last + 1 < lines.end() && lines.wrapped(last) {
        last += 1;
    }
    (first, last)
}

/// The last line that is worth keeping: `chain_last`, or one below it that shows something.
fn last_line(lines: &mut Lines, chain_last: usize) -> usize {
    for line in (chain_last + 1..lines.end()).rev() {
        let shows = (0..lines.columns())
            .any(|column| lines.cell(line, column).is_some_and(|cell| !is_blank(cell)));
        if shows {
            return line;
        }
    }
    chain_last
}

#[cfg(test)]
mod tests {
    use super::super::{CellColor, TerminalScreen};
    use crate::testing::{cases, screen};

    const PROMPT: &str = "macstudio@ubuntu-vm-1:~/work/proj$ ";

    /// The rows of the view, without the blanks that end them.
    fn rows(screen: &TerminalScreen) -> Vec<String> {
        let (rows, _) = screen.size();
        (0..rows)
            .map(|row| {
                let text: String = screen
                    .row_runs(row)
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect();
                text.trim_end().to_owned()
            })
            .collect()
    }

    fn cursor(screen: &TerminalScreen) -> (u16, u16) {
        let cursor = screen.cursor().expect("the cursor is shown");
        (cursor.row, cursor.column)
    }

    #[test]
    fn the_screen_after_a_resize_is_the_one_in_the_vector() {
        // The first case is the one a person saw: the pane made narrower than its prompt and
        // wide again before the shell was told, and the next command typed over the prompt.
        for case in cases("terminal-resize", "reflow") {
            let name = case["name"].as_str().unwrap();
            let size =
                |value: &serde_json::Value| value.as_u64().map(|n| u16::try_from(n).unwrap());
            let mut screen =
                TerminalScreen::new(size(&case["rows"]).unwrap(), size(&case["cols"]).unwrap());
            screen.feed(case["output"].as_str().unwrap().as_bytes());
            for to in case["resizes"].as_array().unwrap() {
                screen.resize(size(&to[0]).unwrap(), size(&to[1]).unwrap());
            }
            let expected: Vec<&str> = case["screen"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row.as_str().unwrap())
                .collect();
            assert_eq!(rows(&screen)[..expected.len()], expected[..], "{name}");
            if let Some(at) = case["cursor"].as_array() {
                let at = (size(&at[0]).unwrap(), size(&at[1]).unwrap());
                assert_eq!(cursor(&screen), at, "{name}");
            }
            for style in case["styles"].as_array().into_iter().flatten() {
                let at = (
                    size(&style["row"]).unwrap(),
                    size(&style["column"]).unwrap(),
                );
                let run = screen
                    .row_runs(at.0)
                    .into_iter()
                    .find(|run| run.column <= at.1 && at.1 < run.column + run.columns)
                    .unwrap();
                let fg = match run.fg {
                    CellColor::Indexed(index) => Some(u64::from(index)),
                    _ => None,
                };
                assert_eq!(fg, style["fg"].as_u64(), "{name} {at:?}");
                assert_eq!(run.bold, style["bold"].as_bool().unwrap(), "{name} {at:?}");
            }
        }
    }

    #[test]
    fn a_pane_made_narrow_and_wide_again_before_anything_arrives_keeps_its_prompt() {
        // What a person saw: the pane was narrower than the prompt for a moment, the shell was
        // never told, and the next command was typed over the middle of the prompt.
        let mut screen = screen(6, 80, &format!("output\r\n{PROMPT}"));
        screen.resize(6, 20);
        assert_eq!(rows(&screen)[..2], ["output", "macstudio@ubuntu-vm-"]);
        assert_eq!(cursor(&screen), (1, 19));
        screen.resize(6, 80);
        assert_eq!(rows(&screen)[..2], ["output", PROMPT.trim_end()]);
        assert_eq!(cursor(&screen), (1, 35));
        screen.feed(b"echo hi");
        assert_eq!(rows(&screen)[1], format!("{PROMPT}echo hi"));
    }

    #[test]
    fn what_was_cut_comes_back_through_any_number_of_sizes_in_between() {
        let mut screen = screen(6, 80, &format!("output\r\n{PROMPT}"));
        for width in [20, 30, 25, 40, 12, 80] {
            screen.resize(6, width);
        }
        assert_eq!(rows(&screen)[1], PROMPT.trim_end());
        assert_eq!(cursor(&screen), (1, 35));
        // Wide enough for it to show whole, but not for the screen it was cut from.
        screen.resize(6, 20);
        screen.resize(6, 40);
        assert_eq!(rows(&screen)[1], PROMPT.trim_end());
        assert_eq!(cursor(&screen), (1, 35));
    }

    #[test]
    fn once_the_shell_has_written_the_screen_it_is_not_put_back() {
        let mut screen = screen(6, 80, &format!("output\r\n{PROMPT}"));
        screen.resize(6, 20);
        // The shell heard of the width and draws its prompt again, which wraps.
        screen.feed(format!("\r\x1b[K{PROMPT}").as_bytes());
        assert_eq!(
            rows(&screen)[..3],
            ["output", "macstudio@ubuntu-vm-", "1:~/work/proj$"]
        );
        screen.resize(6, 80);
        assert_eq!(
            rows(&screen)[..3],
            ["output", "macstudio@ubuntu-vm-", "1:~/work/proj$"]
        );
        assert_eq!(cursor(&screen), (2, 15));
    }

    #[test]
    fn a_cursor_after_a_full_row_is_at_the_start_of_the_next_one() {
        let mut roomy = screen(4, 20, "0123456789abcdefghij\r\n");
        roomy.resize(4, 10);
        assert_eq!(rows(&roomy)[..3], ["0123456789", "abcdefghij", ""]);
        assert_eq!(cursor(&roomy), (2, 0));
        roomy.feed(b"k");
        assert_eq!(rows(&roomy)[2], "k");
        // The same without room below.
        let mut cramped = screen(2, 20, "0123456789abcdefghij\r\n");
        cramped.resize(2, 10);
        assert_eq!(rows(&cramped), ["abcdefghij", ""]);
        assert_eq!(cursor(&cramped), (1, 0));
    }

    #[test]
    fn colors_and_weights_stay_on_their_text() {
        let mut screen = screen(
            6,
            12,
            "\x1b[1;31mred bold\x1b[0m plain \x1b[48;2;1;2;3mbg\x1b[0m\r\n",
        );
        screen.resize(6, 6);
        let first = &screen.row_runs(0)[0];
        assert_eq!(first.text, "red bo");
        assert_eq!(first.fg, CellColor::Indexed(1));
        assert!(first.bold);
        let second = screen.row_runs(1);
        assert_eq!(second[0].text, "ld");
        assert!(second[0].bold);
        assert_eq!(second[1].text, " pla");
        assert!(!second[1].bold);
        assert_eq!(second[1].fg, CellColor::DefaultFg);
        screen.resize(6, 40);
        let runs = screen.row_runs(0);
        assert_eq!(runs[0].text, "red bold");
        assert_eq!(runs[1].text, " plain ");
        assert_eq!(runs[2].text, "bg");
        assert_eq!(runs[2].bg, CellColor::Rgb(1, 2, 3));
    }

    #[test]
    fn a_wide_character_is_never_split_between_rows() {
        let mut screen = screen(5, 10, "ab\u{4f60}\u{597d}cd\r\n");
        screen.resize(5, 3);
        assert_eq!(rows(&screen)[..4], ["ab", "\u{4f60}", "\u{597d}c", "d"]);
        // The parser does not say that a row ended early for a wide character, so the rows it
        // left are not joined again: no text is lost, only that join.
        screen.resize(5, 10);
        assert_eq!(rows(&screen)[..3], ["ab", "\u{4f60}", "\u{597d}cd"]);
        assert_eq!(cursor(&screen), (3, 0));
    }

    #[test]
    fn what_a_program_asked_for_survives_and_so_does_a_hidden_cursor() {
        let mut screen = screen(3, 10, "\x1b[?1h\x1b[?2004h\x1b[?25lhello");
        screen.resize(5, 20);
        assert!(screen.application_cursor());
        assert!(screen.bracketed_paste());
        assert!(screen.cursor().is_none());
    }

    #[test]
    fn the_normal_screen_is_reflowed_under_the_alternate_one_and_back_after_it() {
        let mut screen = screen(6, 30, "a long line of shell output here\r\n$ ");
        screen.feed(b"\x1b[?1049hfull screen");
        screen.resize(6, 10);
        // The program draws its own screen again when it learns the size.
        assert!(screen.contents().trim().is_empty());
        screen.feed(b"\x1b[?1049l");
        assert_eq!(
            rows(&screen)[..4],
            ["a long lin", "e of shell", " output he", "re"]
        );
        screen.resize(6, 40);
        assert_eq!(
            rows(&screen)[..2],
            ["a long line of shell output here", "$"]
        );
    }

    #[test]
    fn blank_rows_below_the_cursor_and_an_empty_screen_are_fine() {
        let mut screen = screen(4, 10, "");
        screen.resize(6, 5);
        assert_eq!(screen.size(), (6, 5));
        assert_eq!(cursor(&screen), (0, 0));
        screen.feed(b"abc\r\n");
        screen.resize(2, 20);
        assert_eq!(rows(&screen), ["abc", ""]);
        assert_eq!(cursor(&screen), (1, 0));
    }

    #[test]
    fn a_sequence_or_a_character_cut_between_two_chunks_goes_on_across_a_resize() {
        let mut screen = screen(3, 10, "\x1b[3");
        screen.resize(3, 12);
        screen.feed(b"1mred");
        assert_eq!(rows(&screen)[0], "red");
        assert_eq!(screen.row_runs(0)[0].fg, CellColor::Indexed(1));

        let mut screen = screen_with(3, 10, b"abc\xe4\xbd");
        screen.resize(3, 12);
        screen.feed(b"\xa0z");
        assert_eq!(rows(&screen)[0], "abc\u{4f60}z");
    }

    fn screen_with(rows: u16, cols: u16, bytes: &[u8]) -> TerminalScreen {
        let mut screen = TerminalScreen::new(rows, cols);
        screen.feed(bytes);
        screen
    }

    #[test]
    fn the_color_in_use_goes_on_across_a_resize() {
        let mut screen = screen(3, 10, "\x1b[31mab");
        screen.resize(3, 12);
        screen.feed(b"cd");
        let runs = screen.row_runs(0);
        assert_eq!(runs[0].text, "abcd");
        assert_eq!(runs[0].fg, CellColor::Indexed(1));
    }

    #[test]
    fn a_view_scrolled_back_stays_scrolled_back() {
        let output: String = (0..30).map(|n| format!("row {n}\r\n")).collect();
        let mut screen = screen(4, 20, &output);
        screen.scroll(5);
        screen.resize(5, 20);
        assert!(screen.scrolled_back());
    }

    #[test]
    fn lines_below_the_cursor_that_wrap_into_more_rows_than_fit_do_not_panic() {
        let mut screen = screen(3, 10, "a\r\nbbbbbbbbbb\r\ncccccccccc\x1b[1;1H");
        screen.resize(3, 2);
        assert_eq!(screen.size(), (3, 2));
        assert_eq!(cursor(&screen), (0, 0));
    }

    #[test]
    fn the_smallest_sizes_are_raised_to_the_least_screen_and_never_panic() {
        for (rows, cols) in [(1, 1), (1, 10), (4, 1), (2, 2)] {
            let mut screen = screen(4, 10, "abcdefghij klm\r\nab\u{4f60}\u{597d}cd\r\n$ ");
            screen.resize(rows, cols);
            assert_eq!(screen.size(), TerminalScreen::fitted(rows, cols));
            screen.resize(4, 10);
            screen.resize(rows, cols);
        }
    }

    /// What a pause in a drag has to pay: the screen is fitted once the size has held for
    /// `SETTLE`, and a full history takes less than that to cut again (measured on a quiet
    /// host: 67 ms to go from 200 to 80 columns). The bound leaves room for a busy one.
    #[test]
    #[ignore = "a timing of the release build: cargo test --release -- --ignored"]
    fn a_full_history_is_fitted_in_a_fraction_of_a_second() {
        let line = "x".repeat(190);
        let output: String = (0..10_020).map(|n| format!("{n:05} {line}\r\n")).collect();
        let mut screen = screen(40, 200, &output);
        for (rows, cols) in [(40, 80), (40, 200), (40, 199), (45, 120)] {
            let start = std::time::Instant::now();
            screen.resize(rows, cols);
            let took = start.elapsed();
            assert!(took.as_millis() < 500, "{cols} columns took {took:?}");
        }
    }

    #[test]
    fn a_full_history_is_reflowed_without_losing_the_text_that_is_kept() {
        let output: String = (0..3000).map(|n| format!("row number {n}\r\n")).collect();
        let mut screen = screen(10, 40, &output);
        screen.resize(10, 7);
        screen.resize(10, 40);
        screen.scroll(i32::MAX);
        let first = rows(&screen)[0].clone();
        assert!(first.starts_with("row number "), "{first}");
        screen.scroll(-i32::MAX);
        assert_eq!(rows(&screen)[8], "row number 2999");
    }
}
