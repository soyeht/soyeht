//! A panic inside the parser costs one screen its text, not the window: `feed` and `resize` catch
//! it, log it once and start that screen again at its size.

use super::{SCROLLBACK_LINES, TerminalScreen};

const WIDE: &str = "\u{4f60}";

/// A screen around a parser of one row and one column, which `TerminalScreen::new` never builds:
/// vt100 0.16 panics when it draws a wide character there.
fn undersized() -> TerminalScreen {
    TerminalScreen::around(vt100::Parser::new(1, 1, SCROLLBACK_LINES))
}

#[test]
fn a_parser_panic_on_output_starts_the_screen_again() {
    let mut screen = undersized();
    screen.feed(WIDE.as_bytes());
    assert!(screen.parser_failed, "the premise: vt100 panics here");
    assert!(screen.contents().trim().is_empty());
    assert!(!screen.scrolled_back());
    // The pane keeps working: the screen takes the next bytes, at a size the parser holds.
    screen.resize(4, 12);
    screen.feed(b"$ ls\r\n");
    assert!(screen.contents().contains("$ ls"));
    assert_eq!(screen.size(), (4, 12));
}

#[test]
fn a_selection_does_not_survive_a_screen_that_started_again() {
    let mut screen = undersized();
    screen.begin_selection(0, 0, 1);
    screen.feed(WIDE.as_bytes());
    assert!(screen.selected_text().is_none());
}

#[test]
fn a_parser_panic_on_resize_starts_the_screen_again_at_the_new_size() {
    let mut screen = undersized();
    screen.feed(b"ab");
    screen.resize(5, 9);
    // Whether this size change panics depends on the parser; either way the screen is usable.
    screen.feed(b"x\r\n");
    assert_eq!(screen.size(), (5, 9));
    assert!(screen.contents().contains('x'));
}

/// A small xorshift: the same sequence on every run and no new dependency.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// Pieces of output of every kind: text, wide and combining characters, line breaks, and the
/// sequences that move the cursor, scroll, set a region, switch screens or toggle wrapping.
const PIECES: [&str; 28] = [
    "a",
    "hello world",
    "\u{4f60}",
    "\u{4f60}\u{597d}\u{4f60}",
    "\u{1f600}",
    "e\u{301}",
    "\u{200b}",
    "\r\n",
    "\n",
    "\r",
    "\t",
    "\u{8}",
    "\x1b[2;3r",
    "\x1b[3S",
    "\x1b[2T",
    "\x1b[2L",
    "\x1b[3M",
    "\x1bM",
    "\x1bD",
    "\x1b[?1049h",
    "\x1b[?1049l",
    "\x1b[9;9H",
    "\x1b[3@",
    "\x1b[2P",
    "\x1b[?7l",
    "\x1b[?7h",
    "\x1b7\x1b8",
    "\x1b[1;31m",
];

#[test]
fn random_output_and_resizes_never_panic_the_parser() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for _ in 0..400 {
        let mut screen =
            TerminalScreen::new(1 + random.below(6) as u16, 1 + random.below(6) as u16);
        for _ in 0..40 {
            if random.below(4) == 0 {
                screen.resize(random.below(7) as u16, random.below(7) as u16);
            } else {
                let mut bytes = Vec::new();
                for _ in 0..1 + random.below(6) {
                    bytes.extend_from_slice(PIECES[random.below(PIECES.len())].as_bytes());
                }
                // Cut anywhere, also inside a character or a sequence, as a read of the pty does.
                let cut = random.below(bytes.len() + 1);
                screen.feed(&bytes[..cut]);
                screen.feed(&bytes[cut..]);
            }
        }
        // Contained is not enough: a panic the screen caught is a bug the parser still has.
        assert!(!screen.parser_failed, "{:?}", screen.size());
    }
}
