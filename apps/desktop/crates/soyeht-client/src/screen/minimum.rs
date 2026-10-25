//! A pane the engine holds at one row or one column, such as one of eight stacked in a column,
//! must not take the window down: vt100 panics on a screen that small (see `MIN_ROWS`).

use super::{MIN_COLUMNS, MIN_ROWS, TerminalScreen};
use crate::testing::screen;

const WIDE: &str = "\u{4f60}";

fn rows(screen: &TerminalScreen) -> Vec<String> {
    let (rows, _) = screen.size();
    (0..rows)
        .map(|row| {
            screen
                .row_runs(row)
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .map(|row| row.trim_end().to_owned())
        .collect()
}

#[test]
fn a_screen_is_never_smaller_than_the_parser_holds() {
    assert_eq!(TerminalScreen::fitted(1, 1), (MIN_ROWS, MIN_COLUMNS));
    assert_eq!(TerminalScreen::fitted(0, 40), (MIN_ROWS, 40));
    assert_eq!(TerminalScreen::fitted(24, 80), (24, 80));
    assert_eq!(TerminalScreen::new(1, 1).size(), (MIN_ROWS, MIN_COLUMNS));
    let mut screen = TerminalScreen::new(24, 80);
    screen.resize(1, 1);
    assert_eq!(screen.size(), (MIN_ROWS, MIN_COLUMNS));
    screen.resize(0, 0);
    assert_eq!(screen.size(), (MIN_ROWS, MIN_COLUMNS));
}

#[test]
fn a_long_line_on_one_row_wraps() {
    let screen = screen(1, 10, "0123456789abcdef\r\n");
    assert_eq!(screen.size().1, 10);
    assert!(screen.contents().contains("abcdef"));
}

#[test]
fn a_long_line_on_one_row_and_five_columns_wraps() {
    let screen = screen(1, 5, "abcdefghij");
    // The text is whole: contents() joins the rows a line wrapped into.
    assert_eq!(screen.contents(), "abcdefghij");
    assert_eq!(rows(&screen), ["abcde", "fghij"]);
}

#[test]
fn wide_characters_fit_on_one_row_and_two_columns() {
    let screen = screen(1, 2, &WIDE.repeat(3));
    assert_eq!(screen.size().1, 2);
    assert!(screen.contents().contains(WIDE));
}

#[test]
fn a_wide_character_on_a_screen_of_three_rows_and_one_column_is_drawn() {
    let screen = screen(3, 1, WIDE);
    assert_eq!(screen.size(), (3, MIN_COLUMNS));
    assert_eq!(screen.text_at(0, 0), WIDE);
}

#[test]
fn long_output_after_a_resize_to_one_row_is_kept() {
    let mut screen = screen(24, 80, "one\r\ntwo\r\n$ ");
    screen.resize(1, 80);
    screen.feed("x".repeat(200).as_bytes());
    screen.feed(b"\r\n");
    screen.feed("y".repeat(300).as_bytes());
    assert_eq!(screen.size(), (MIN_ROWS, 80));
    assert!(screen.contents().contains("yyy"));
    screen.resize(24, 80);
    screen.resize(1, 1);
    screen.feed(WIDE.repeat(50).as_bytes());
}

#[test]
fn the_cursor_and_the_selection_stay_inside_the_screen() {
    let mut screen = screen(1, 10, "hello world");
    let (screen_rows, screen_columns) = screen.size();
    let cursor = screen.cursor().expect("the cursor shows");
    assert!(cursor.row < screen_rows && cursor.column < screen_columns);
    screen.begin_selection(0, 0, 1);
    screen.extend_selection(0, 4);
    assert_eq!(screen.selected_text().as_deref(), Some("hello"));
    assert_eq!(screen.selection_columns(0), Some((0, 5)));
}

#[test]
fn growing_back_from_the_smallest_screen_keeps_the_text() {
    let mut screen = screen(6, 20, "abc\r\ndef\r\n$ ");
    screen.resize(1, 1);
    screen.resize(6, 20);
    assert_eq!(rows(&screen)[..2], ["abc", "def"]);
}

/// Input of every kind the parser handles: ASCII, wide characters, line breaks, and the escape
/// sequences that move the cursor, scroll or set a region.
const OUTPUTS: [&str; 7] = [
    "0123456789abcdef\r\n",
    "\u{4f60}\u{597d}\u{4f60}\u{597d}\u{4f60}\u{597d}",
    "ab\u{4f60}cd\r\n\u{4f60}\n\r\nx",
    "\u{1f600}e\u{301}\t\u{8}z\r\n",
    "\x1b[2;3r\x1b[5S\x1b[3T\x1b[2L\x1b[3M\x1bM\x1bD\nabcdef",
    "\x1b[?1049h\u{4f60}\u{4f60}\x1b[9;9Hq\x1b[?1049l\x1b[9D\x1b[3@\x1b[2P",
    "\x1b[?7l\u{4f60}\u{4f60}\u{4f60}\x1b[?7habcdefgh\x1b8\x1b7\u{4f60}",
];

#[test]
fn no_size_from_1x1_to_4x6_panics_on_mixed_output() {
    for rows in 1..=4 {
        for cols in 1..=6 {
            for output in OUTPUTS {
                let mut whole = TerminalScreen::new(rows, cols);
                whole.feed(output.as_bytes());
                let mut chunked = TerminalScreen::new(rows, cols);
                for chunk in output.as_bytes().chunks(3) {
                    chunked.feed(chunk);
                }
                assert_eq!(
                    whole.contents(),
                    chunked.contents(),
                    "{rows}x{cols} {output:?}"
                );
            }
        }
    }
}

#[test]
fn no_resize_between_1x1_and_4x6_panics_while_output_goes_on() {
    let sizes: Vec<(u16, u16)> = (1..=4)
        .flat_map(|rows| (1..=6).map(move |cols| (rows, cols)))
        .collect();
    for (from, to) in sizes
        .iter()
        .flat_map(|from| sizes.iter().map(move |to| (*from, *to)))
    {
        for output in OUTPUTS {
            let mut screen = TerminalScreen::new(from.0, from.1);
            screen.feed(output.as_bytes());
            screen.resize(to.0, to.1);
            screen.feed(output.as_bytes());
            screen.resize(from.0, from.1);
            screen.feed(output.as_bytes());
            assert_eq!(screen.size(), TerminalScreen::fitted(from.0, from.1));
        }
    }
}
