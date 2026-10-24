//! What a pointer does to a terminal screen: select text, find the address under it. Rows here
//! are rows of the view; inside, places are lines from the oldest one kept (see `lines`).

use super::{
    Lines, SCROLLBACK_LINES, TerminalScreen,
    links::{self, Link},
    selection::{Point, Selection, Span, Unit},
};

/// What decides whether output left the selected text alone.
pub(super) struct Guard {
    /// The lines from this one on were on the live screen, where output can change them.
    live_from: usize,
    print: String,
    alternate: bool,
}

impl TerminalScreen {
    /// The line shown in a row of the view.
    fn line_at(&self, row: u16) -> usize {
        self.history
            .saturating_sub(self.parser.screen().scrollback())
            + usize::from(row)
    }

    /// A press at a cell of the view; `clicks` is how many presses in a row this one is.
    pub fn begin_selection(&mut self, row: u16, column: u16, clicks: usize) {
        let at = Point {
            line: self.line_at(row),
            column,
        };
        let mut lines = Lines::new(&mut self.parser);
        self.selection = Some(Selection::begin(&mut lines, at, Unit::of_clicks(clicks)));
    }

    /// The pointer moved, with the button down, to a cell of the view. It grows the selection the
    /// press began, if output has not ended it.
    pub fn extend_selection(&mut self, row: u16, column: u16) {
        let at = Point {
            line: self.line_at(row),
            column,
        };
        // A selection that output ended while the pointer was down is not begun again where the
        // pointer is: the drag that made it is over.
        let Some(selection) = self.selection.as_mut() else {
            return;
        };
        let mut lines = Lines::new(&mut self.parser);
        selection.extend(&mut lines, at);
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Whether any text is selected.
    pub fn has_selection(&self) -> bool {
        self.selection
            .is_some_and(|selection| selection.span().is_some())
    }

    /// The selected text, ready for the clipboard.
    pub fn selected_text(&mut self) -> Option<String> {
        let selection = self.selection?;
        let mut lines = Lines::new(&mut self.parser);
        selection.text(&mut lines).filter(|text| !text.is_empty())
    }

    /// The columns of a row of the view that are selected, from the first to one past the last.
    pub fn selection_columns(&self, row: u16) -> Option<(u16, u16)> {
        self.columns_of(self.selection?.span()?, row)
    }

    /// The columns of a row of the view that `span` covers, as [`Self::selection_columns`].
    pub fn columns_of(&self, span: Span, row: u16) -> Option<(u16, u16)> {
        let (_, columns) = self.size();
        span.columns_in(self.line_at(row), columns)
    }

    /// The web address shown at a cell of the view.
    pub fn link_at(&mut self, row: u16, column: u16) -> Option<Link> {
        let at = Point {
            line: self.line_at(row),
            column,
        };
        let mut lines = Lines::new(&mut self.parser);
        links::link_at(&mut lines, at)
    }

    /// Taken before output is applied.
    pub(super) fn selection_guard(&mut self) -> Option<Guard> {
        let selection = self.selection?;
        let alternate = self.parser.screen().alternate_screen();
        let mut lines = Lines::new(&mut self.parser);
        let live_from = lines.history();
        let print = fingerprint(&selection, &mut lines, live_from);
        Some(Guard {
            live_from,
            print,
            alternate,
        })
    }

    /// Output changed the text under the selection, or moved it: the selection no longer
    /// says what the person chose, so it goes.
    pub(super) fn keep_selection_if_intact(&mut self, guard: Option<Guard>) {
        let (Some(guard), Some(selection)) = (guard, self.selection) else {
            return;
        };
        let alternate = self.parser.screen().alternate_screen();
        let mut lines = Lines::new(&mut self.parser);
        if alternate != guard.alternate
            || fingerprint(&selection, &mut lines, guard.live_from) != guard.print
        {
            self.selection = None;
        }
    }
}

/// The text output can change under a selection. Once the history is full every new line pushes
/// the oldest out and moves all the places, so the lines at its edge are part of it: they say
/// whether anything scrolled.
fn fingerprint(selection: &Selection, lines: &mut Lines, live_from: usize) -> String {
    let mut print = selection.live_text(lines, live_from);
    if live_from >= SCROLLBACK_LINES {
        let last = lines.columns().saturating_sub(1);
        for line in live_from - 1..=live_from {
            print.push('\u{0}');
            print.push_str(&lines.text(line, 0, last));
        }
    }
    print
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::screen;

    fn select(screen: &mut TerminalScreen, from: (u16, u16), to: (u16, u16)) -> Option<String> {
        screen.begin_selection(from.0, from.1, 1);
        screen.extend_selection(to.0, to.1);
        screen.selected_text()
    }

    #[test]
    fn a_drag_selects_the_cells_between_its_ends_inclusive() {
        let mut screen = screen(3, 20, "hello world\r\nsecond line");
        assert_eq!(
            select(&mut screen, (0, 2), (0, 7)).as_deref(),
            Some("llo wo")
        );
        assert_eq!(screen.selection_columns(0), Some((2, 8)));
        assert_eq!(screen.selection_columns(1), None);
        // Dragging backwards selects the same cells.
        assert_eq!(
            select(&mut screen, (0, 7), (0, 2)).as_deref(),
            Some("llo wo")
        );
    }

    #[test]
    fn a_drag_over_rows_takes_the_ends_of_the_first_and_whole_rows_between() {
        let mut screen = screen(4, 20, "first line\r\nsecond line\r\nthird line");
        assert_eq!(
            select(&mut screen, (0, 6), (2, 4)).as_deref(),
            Some("line\nsecond line\nthird")
        );
        assert_eq!(screen.selection_columns(0), Some((6, 20)));
        assert_eq!(screen.selection_columns(1), Some((0, 20)));
        assert_eq!(screen.selection_columns(2), Some((0, 5)));
        assert_eq!(screen.selection_columns(3), None);
    }

    #[test]
    fn a_press_without_a_drag_selects_nothing() {
        let mut screen = screen(2, 20, "hello");
        screen.begin_selection(0, 1, 1);
        assert!(!screen.has_selection());
        assert_eq!(screen.selected_text(), None);
        screen.extend_selection(0, 1);
        assert!(!screen.has_selection());
        screen.extend_selection(0, 2);
        assert!(screen.has_selection());
        screen.clear_selection();
        assert!(!screen.has_selection());
        assert_eq!(screen.selection_columns(0), None);
    }

    #[test]
    fn a_double_click_selects_a_word_and_a_drag_grows_it_by_words() {
        let mut screen = screen(2, 40, "cd ~/src/soyeht && ls -la");
        screen.begin_selection(0, 5, 2);
        assert_eq!(screen.selected_text().as_deref(), Some("~/src/soyeht"));
        screen.extend_selection(0, 17);
        assert_eq!(screen.selected_text().as_deref(), Some("~/src/soyeht &&"));
        screen.extend_selection(0, 1);
        assert_eq!(screen.selected_text().as_deref(), Some("cd ~/src/soyeht"));
    }

    #[test]
    fn a_double_click_on_blanks_or_punctuation_takes_the_run_of_the_same_kind() {
        let mut screen = screen(1, 40, "a   (b)");
        screen.begin_selection(0, 2, 2);
        assert_eq!(screen.selection_columns(0), Some((1, 4)));
        screen.begin_selection(0, 4, 2);
        assert_eq!(screen.selected_text().as_deref(), Some("("));
    }

    #[test]
    fn a_triple_click_selects_the_whole_line_even_where_it_wraps() {
        let mut screen = screen(8, 10, "short\r\nthis line is long enough to wrap\r\nlast");
        screen.begin_selection(1, 3, 3);
        assert_eq!(
            screen.selected_text().as_deref(),
            Some("this line is long enough to wrap")
        );
        assert_eq!(screen.selection_columns(1), Some((0, 10)));
        assert_eq!(screen.selection_columns(4), Some((0, 10)));
        assert_eq!(screen.selection_columns(0), None);
        assert_eq!(screen.selection_columns(5), None);
        screen.begin_selection(0, 0, 3);
        assert_eq!(screen.selected_text().as_deref(), Some("short"));
    }

    #[test]
    fn a_word_that_wraps_is_selected_whole() {
        let mut screen = screen(5, 10, "see https://example.com/long here");
        screen.begin_selection(1, 2, 2);
        assert_eq!(
            screen.selected_text().as_deref(),
            Some("https://example.com/long")
        );
    }

    #[test]
    fn wide_characters_are_selected_whole_and_copied_once() {
        let mut screen = screen(1, 12, "a \u{4f60}\u{597d} b");
        // The second half of a wide character selects it.
        screen.begin_selection(0, 3, 2);
        assert_eq!(screen.selected_text().as_deref(), Some("\u{4f60}\u{597d}"));
        assert_eq!(screen.selection_columns(0), Some((2, 6)));
        assert_eq!(
            select(&mut screen, (0, 0), (0, 3)).as_deref(),
            Some("a \u{4f60}")
        );
        assert_eq!(screen.selection_columns(0), Some((0, 4)));
        assert_eq!(
            select(&mut screen, (0, 3), (0, 4)).as_deref(),
            Some("\u{4f60}\u{597d}")
        );
    }

    #[test]
    fn a_drag_past_the_text_does_not_copy_the_blanks() {
        let mut screen = screen(2, 20, "abc\r\ndef");
        assert_eq!(
            select(&mut screen, (0, 1), (1, 15)).as_deref(),
            Some("bc\ndef")
        );
    }

    #[test]
    fn the_selection_stays_on_its_text_while_the_view_scrolls() {
        let mut output = String::from("keep this\r\n");
        output.push_str(
            &(0..10)
                .map(|n| format!("filler {n}\r\n"))
                .collect::<String>(),
        );
        let mut screen = screen(4, 20, &output);
        screen.scroll(100);
        assert_eq!(select(&mut screen, (0, 0), (0, 3)).as_deref(), Some("keep"));
        // The view scrolling does not move it.
        screen.scroll(-100);
        assert_eq!(screen.selection_columns(0), None);
        screen.scroll(100);
        assert_eq!(screen.selection_columns(0), Some((0, 4)));
        assert_eq!(screen.selected_text().as_deref(), Some("keep"));
    }

    #[test]
    fn a_selection_across_the_history_and_the_screen_is_read_whole() {
        let output: String = (0..12).map(|n| format!("row {n}\r\n")).collect();
        let mut screen = screen(4, 20, &output);
        // Select from a line in the history to one on the screen, from a view that shows both.
        screen.scroll(2);
        screen.begin_selection(0, 0, 1);
        screen.scroll(-2);
        screen.extend_selection(1, 5);
        assert_eq!(
            screen.selected_text().as_deref(),
            Some("row 7\nrow 8\nrow 9\nrow 10")
        );
    }

    #[test]
    fn reading_a_selection_leaves_the_view_where_it_was() {
        let output: String = (0..30).map(|n| format!("row {n}\r\n")).collect();
        let mut screen = screen(4, 20, &output);
        screen.scroll(5);
        screen.begin_selection(0, 0, 3);
        let before = screen.contents();
        screen.selected_text();
        screen.link_at(0, 0);
        assert_eq!(screen.contents(), before);
    }

    #[test]
    fn output_that_rewrites_the_selected_text_clears_the_selection() {
        let mut screen = screen(3, 20, "progress 10%");
        assert!(select(&mut screen, (0, 9), (0, 11)).is_some());
        // The same cells change: the selection no longer says what was chosen.
        screen.feed(b"\rprogress 20%");
        assert!(!screen.has_selection());
    }

    #[test]
    fn output_elsewhere_leaves_the_selection_alone() {
        let mut screen = screen(4, 20, "keep\r\nspinner |");
        assert_eq!(select(&mut screen, (0, 0), (0, 3)).as_deref(), Some("keep"));
        screen.feed(b"\x1b[2;9H/");
        assert_eq!(screen.selected_text().as_deref(), Some("keep"));
    }

    #[test]
    fn a_line_feed_in_the_output_clears_the_selection_wherever_it_is() {
        // The three bytes SwiftTerm, the Mac client's terminal, calls a line feed; a carriage
        // return, a cursor move and plain text are not one.
        for (output, kept) in [
            ("\n", false),
            ("\r\n", false),
            ("\x0b", false),
            ("\x0c", false),
            ("text\r\nmore", false),
            ("\r", true),
            ("\x1b[3;1Hmore", true),
            ("more", true),
        ] {
            for scrolled_back in [0, 100] {
                let mut screen = screen(3, 20, "one\r\ntwo\r\nthree\r\nfour\r\nfive");
                screen.scroll(scrolled_back);
                assert!(select(&mut screen, (0, 0), (0, 2)).is_some());
                screen.feed(output.as_bytes());
                assert_eq!(
                    screen.has_selection(),
                    kept,
                    "{output:?}, scrolled back {scrolled_back}"
                );
            }
        }
    }

    #[test]
    fn a_line_feed_that_scrolls_the_selected_text_into_the_history_clears_it() {
        let mut screen = screen(3, 20, "one\r\ntwo\r\nthree");
        assert_eq!(select(&mut screen, (0, 0), (0, 2)).as_deref(), Some("one"));
        // The new line pushes "one" up: the selection does not stay on the cells it was drawn on.
        screen.feed(b"\r\nfour");
        assert!(!screen.has_selection());
        assert_eq!(screen.selection_columns(0), None);
    }

    #[test]
    fn a_resize_or_a_reset_clears_the_selection() {
        let mut screen = screen(3, 20, "abc def");
        select(&mut screen, (0, 0), (0, 3));
        screen.resize(3, 20);
        assert!(screen.has_selection());
        screen.resize(5, 30);
        assert!(!screen.has_selection());
        select(&mut screen, (0, 0), (0, 3));
        screen.reset();
        assert!(!screen.has_selection());
    }

    #[test]
    fn switching_to_the_alternate_screen_clears_the_selection() {
        let mut screen = screen(3, 20, "abc def");
        select(&mut screen, (0, 0), (0, 3));
        screen.feed(b"\x1b[?1049h");
        assert!(!screen.has_selection());
    }

    #[test]
    fn once_the_history_is_full_scrolling_output_clears_a_selection_in_it() {
        let output: String = (0..SCROLLBACK_LINES + 20)
            .map(|n| format!("row {n}\r\n"))
            .collect();
        let mut screen = screen(4, 20, &output);
        screen.scroll(1000);
        assert!(select(&mut screen, (0, 0), (0, 3)).is_some());
        screen.feed(b"newer\r\n");
        assert!(!screen.has_selection());
    }

    #[test]
    fn output_at_the_bottom_leaves_a_selection_in_the_history_alone() {
        let output: String = (0..20).map(|n| format!("row {n}\r\n")).collect();
        let mut screen = screen(4, 20, &output);
        screen.scroll(100);
        assert!(select(&mut screen, (0, 0), (0, 4)).is_some());
        screen.feed(b"x");
        assert_eq!(screen.selected_text().as_deref(), Some("row 0"));
    }

    #[test]
    fn a_drag_that_output_ended_is_not_begun_again_where_the_pointer_is() {
        let mut screen = screen(4, 20, "one\r\ntwo");
        screen.begin_selection(0, 0, 1);
        screen.extend_selection(0, 2);
        assert!(screen.has_selection());
        screen.feed(b"\r\nmore");
        assert!(!screen.has_selection());
        screen.extend_selection(1, 1);
        screen.extend_selection(1, 2);
        assert!(!screen.has_selection());
        // The next press begins one as usual.
        screen.begin_selection(1, 0, 1);
        screen.extend_selection(1, 2);
        assert_eq!(screen.selected_text().as_deref(), Some("two"));
    }
}
