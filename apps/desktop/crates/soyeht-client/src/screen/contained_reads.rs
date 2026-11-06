//! A panic inside the parser while the screen is read or its view is moved costs that screen its
//! text, not the window: the same containment as `feed` and `resize` (see `contained`). The tests
//! make the parser's work panic by giving the guarded call a closure that does.

use super::{Lines, TerminalScreen};

fn screen_with_text() -> TerminalScreen {
    let mut screen = TerminalScreen::new(4, 12);
    screen.feed(b"one\r\ntwo\r\nthree");
    screen
}

#[test]
fn a_read_that_panics_shows_nothing_and_is_logged_once() {
    let screen = screen_with_text();
    assert!(screen.contents().contains("three"));

    let shown: String = screen.read("fallback".to_owned(), |_| panic!("a parser with a bug"));
    assert_eq!(shown, "fallback");
    assert!(screen.parser_failed.get());
    assert!(screen.damaged.get());

    // Until the screen can start again it shows nothing, without asking the parser.
    assert!(screen.contents().is_empty());
    assert!(screen.row_runs_within(0, 12).is_empty());
}

#[test]
fn the_next_write_starts_a_screen_with_a_failed_read_again() {
    let mut screen = screen_with_text();
    let _: () = screen.read((), |_| panic!("a parser with a bug"));
    screen.feed(b"$ ls\r\n");
    assert!(!screen.damaged.get());
    assert!(screen.contents().contains("$ ls"));
    assert!(!screen.contents().contains("three"));
    assert_eq!(screen.size(), (4, 12));
}

#[test]
fn a_move_of_the_view_that_panics_starts_the_screen_again() {
    let mut screen = screen_with_text();
    for line in 0..20 {
        screen.feed(format!("line {line}\r\n").as_bytes());
    }
    screen.begin_selection(0, 0, 1);
    screen.scroll(5);
    assert!(screen.scrolled_back());

    let moved = screen.contained(false, |_| panic!("a parser with a bug"));
    assert!(!moved);
    assert!(screen.parser_failed.get());
    assert!(!screen.scrolled_back());
    assert!(!screen.has_selection());
    assert_eq!(screen.size(), (4, 12));
    screen.feed(b"back\r\n");
    assert!(screen.contents().contains("back"));
}

#[test]
fn lines_that_are_dropped_while_a_panic_unwinds_do_not_raise_another() {
    let mut screen = screen_with_text();
    let history = screen.contained(usize::MAX, |screen| {
        let _lines = Lines::new(&mut screen.parser);
        panic!("a parser with a bug");
    });
    assert_eq!(history, usize::MAX);
    assert!(screen.parser_failed.get());
}

#[test]
fn reads_and_moves_that_do_not_panic_are_unchanged() {
    let mut screen = screen_with_text();
    assert_eq!(screen.text_at(0, 0), "o");
    assert_eq!(screen.row_runs_within(1, 12)[0].text.trim_end(), "two");
    screen.scroll(1);
    screen.scroll_to_bottom();
    assert!(!screen.scrolled_back());
    assert!(!screen.parser_failed.get());
    assert!(!screen.damaged.get());
}

/// The reads of the parser that `feed` makes around its writing (the selection's fingerprint
/// before and after, the count of the history) are contained too: the screen starts again and the
/// bytes of the write are not lost to the panic of a read.
mod during_a_write {
    use super::TerminalScreen;

    fn with_a_selection() -> TerminalScreen {
        let mut screen = TerminalScreen::new(4, 12);
        screen.feed(b"hello world");
        screen.begin_selection(0, 0, 1);
        screen.extend_selection(0, 4);
        assert!(screen.has_selection());
        screen
    }

    fn assert_started_again(screen: &TerminalScreen) {
        assert!(screen.parser_failed.get());
        assert!(!screen.damaged.get());
        assert!(!screen.has_selection());
        assert_eq!(screen.size(), (4, 12));
    }

    #[test]
    fn a_panic_counting_the_history_starts_the_screen_again() {
        let mut screen = TerminalScreen::new(4, 12);
        screen.fault_after.set(Some(0));
        screen.feed(b"hello");
        assert_started_again(&screen);
        screen.feed(b"back");
        assert!(screen.contents().contains("back"));
    }

    #[test]
    fn a_panic_taking_the_fingerprint_of_the_selection_starts_the_screen_again() {
        let mut screen = with_a_selection();
        screen.fault_after.set(Some(0));
        screen.feed(b"!");
        assert_started_again(&screen);
        // The bytes of the write reach the screen that started again.
        assert_eq!(screen.contents(), "!");
    }

    #[test]
    fn a_panic_comparing_the_selection_after_the_write_starts_the_screen_again() {
        let mut screen = with_a_selection();
        // The fingerprint before and the count of the history come first.
        screen.fault_after.set(Some(2));
        screen.feed(b"!");
        assert_started_again(&screen);
        assert!(screen.contents().trim().is_empty());
        screen.feed(b"back");
        assert!(screen.contents().contains("back"));
    }
}
