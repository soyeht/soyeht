//! Links in terminal output: web addresses and file references. Only `http` and `https`
//! addresses are links: a terminal prints text from programs and files it does not control, and
//! an address of another scheme (`file:`, `ssh:`, an application's own) would start another
//! program. A file reference (`src/main.rs:12`, the rule of [`crate::file_links`]) is found here
//! by its text; whether the file is in the workspace is the window's to say.

use super::{
    lines::{Glyph, Lines, Logical},
    selection::{Point, Span},
};
use crate::file_links::{self, Dialect, Reference};

/// The schemes of the addresses that open, with what follows them.
const SCHEMES: [&str; 2] = ["https://", "http://"];

/// Characters that end an address.
const ENDS_ADDRESS: &str = "<>\"`\\";

/// Characters an address may end in only when they are part of it: `.` at the end of a sentence
/// is not.
const TRAILING_PUNCTUATION: &str = ".,;:!?'*";

/// A web address or a file reference in the output, and the cells it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The text of the link: the address, or the reference as it is written.
    pub url: String,
    pub span: Span,
    /// What a file reference points at; `None` for a web address.
    pub file: Option<Reference>,
}

/// The link that takes the cell at `at`, if there is one.
pub(crate) fn link_at(lines: &mut Lines, at: Point) -> Option<Link> {
    let logical = Logical::around(lines, at.line);
    let index = logical.index_of(at.line, at.column)?;
    let glyphs = &logical.glyphs;
    let characters: Vec<char> = glyphs
        .iter()
        .map(|glyph| glyph.character().unwrap_or('\0'))
        .collect();
    let mut start = 0;
    while start <= index {
        // A scheme in the middle of a word (`xhttp://`) is not the start of an address.
        let inside_word = start > 0 && characters[start - 1].is_alphanumeric();
        let length = if inside_word {
            None
        } else {
            address_length(&characters[start..])
        };
        let Some(length) = length else {
            start += 1;
            continue;
        };
        if index < start + length {
            return Some(link_of(glyphs, start, length));
        }
        // Past the address, the next one starts after it.
        start += length.max(1);
    }
    let found = file_links::find_at(Dialect::HOST, &characters, index)?;
    let mut link = link_of(glyphs, found.range.start, found.range.len());
    link.file = Some(found.reference);
    Some(link)
}

fn link_of(glyphs: &[Glyph], start: usize, length: usize) -> Link {
    let words = &glyphs[start..start + length];
    let last = &words[length - 1];
    Link {
        url: words.iter().map(|glyph| glyph.text.as_str()).collect(),
        span: Span {
            start: Point {
                line: words[0].line,
                column: words[0].column,
            },
            end: Point {
                line: last.line,
                column: last.last_column(),
            },
        },
        file: None,
    }
}

/// How many characters of `text` are a web address that starts at its first one.
fn address_length(text: &[char]) -> Option<usize> {
    let scheme = SCHEMES.iter().find(|scheme| starts_with(text, scheme))?;
    let mut end = text
        .iter()
        .position(|&character| ends_address(character))
        .unwrap_or(text.len());
    while end > scheme.len() && closes_early(&text[..end]) {
        end -= 1;
    }
    // There must be a host: something between the scheme and the first slash, query or fragment.
    let host = text[scheme.len()..end]
        .iter()
        .take_while(|&&character| !"/?#".contains(character))
        .any(|character| character.is_alphanumeric());
    host.then_some(end)
}

fn starts_with(text: &[char], prefix: &str) -> bool {
    let mut characters = text.iter();
    prefix.chars().all(|expected| {
        characters
            .next()
            .is_some_and(|character| character.eq_ignore_ascii_case(&expected))
    })
}

fn ends_address(character: char) -> bool {
    character.is_whitespace() || character.is_control() || ENDS_ADDRESS.contains(character)
}

/// Whether the last character of `address` belongs to the text around it: a full stop, or a
/// bracket that closes one the address never opened.
fn closes_early(address: &[char]) -> bool {
    let Some(&last) = address.last() else {
        return false;
    };
    if TRAILING_PUNCTUATION.contains(last) {
        return true;
    }
    let opening = match last {
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => return false,
    };
    let count = |wanted: char| {
        address
            .iter()
            .filter(|&&character| character == wanted)
            .count()
    };
    count(last) > count(opening)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::TerminalScreen;

    fn url_in(rows: u16, cols: u16, output: &str, row: u16, column: u16) -> Option<String> {
        let mut screen = TerminalScreen::new(rows, cols);
        screen.feed(output.as_bytes());
        screen.link_at(row, column).map(|link| link.url)
    }

    fn text(address: &str) -> Vec<char> {
        address.chars().collect()
    }

    #[test]
    fn an_address_in_a_sentence_is_found_without_its_punctuation() {
        let output = "see https://example.com/a?b=1#c. Then";
        assert_eq!(
            url_in(2, 60, output, 0, 8).as_deref(),
            Some("https://example.com/a?b=1#c")
        );
        assert_eq!(url_in(2, 60, output, 0, 1), None);
        // The full stop is not part of the link.
        assert_eq!(url_in(2, 60, output, 0, 31), None);
    }

    #[test]
    fn brackets_around_an_address_are_not_part_of_it_but_balanced_ones_are() {
        assert_eq!(
            url_in(1, 60, "(see http://example.org/x)", 0, 8).as_deref(),
            Some("http://example.org/x")
        );
        assert_eq!(
            url_in(1, 60, "http://example.org/a_(b)", 0, 3).as_deref(),
            Some("http://example.org/a_(b)")
        );
        assert_eq!(
            url_in(1, 60, "<https://example.com>", 0, 3).as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn the_scheme_may_be_in_any_case() {
        assert_eq!(
            url_in(1, 40, "HTTPS://EXAMPLE.COM/x", 0, 0).as_deref(),
            Some("HTTPS://EXAMPLE.COM/x")
        );
    }

    #[test]
    fn two_addresses_on_a_row_are_told_apart() {
        let output = "http://a.example http://b.example";
        assert_eq!(
            url_in(1, 60, output, 0, 2).as_deref(),
            Some("http://a.example")
        );
        assert_eq!(
            url_in(1, 60, output, 0, 20).as_deref(),
            Some("http://b.example")
        );
        assert_eq!(url_in(1, 60, output, 0, 16), None);
    }

    #[test]
    fn an_address_that_wraps_is_one_link_on_every_row() {
        let output = "x https://example.com/a/long/path/that/wraps";
        let first = url_in(4, 20, output, 0, 5);
        let second = url_in(4, 20, output, 1, 5);
        assert_eq!(
            first.as_deref(),
            Some("https://example.com/a/long/path/that/wraps")
        );
        assert_eq!(first, second);
        let mut screen = TerminalScreen::new(4, 20);
        screen.feed(output.as_bytes());
        let link = screen.link_at(1, 0).unwrap();
        assert_eq!(link.span.start, Point { line: 0, column: 2 });
        assert_eq!(link.span.end.line, 2);
    }

    #[test]
    fn only_web_schemes_are_links() {
        for output in [
            "file:///etc/passwd",
            "ssh://host/x",
            "ftp://example.com",
            "javascript:alert(1)",
            "soyeht://pair?code=1",
            "mailto:a@example.com",
            "xhttp://example.com",
        ] {
            let last = u16::try_from(output.len() - 1).unwrap();
            for column in 0..=last {
                assert_eq!(
                    url_in(1, 60, output, 0, column),
                    None,
                    "{output} column {column}"
                );
            }
        }
    }

    #[test]
    fn a_file_reference_is_a_link_with_what_it_points_at() {
        let output = "error at src/main.rs:12:5 here";
        let mut screen = TerminalScreen::new(2, 60);
        screen.feed(output.as_bytes());
        let link = screen.link_at(0, 14).unwrap();
        assert_eq!(link.url, "src/main.rs:12:5");
        assert_eq!(link.span.start, Point { line: 0, column: 9 });
        assert_eq!(
            link.span.end,
            Point {
                line: 0,
                column: 24
            }
        );
        let reference = link.file.unwrap();
        assert_eq!(reference.path, "src/main.rs");
        assert_eq!((reference.line, reference.column), (11, Some(4)));
        // The words around it are not part of it.
        assert_eq!(url_in(2, 60, output, 0, 3), None);
        assert!(screen.link_at(0, 26).is_none());
    }

    #[test]
    fn a_web_address_with_a_port_is_still_an_address() {
        let output = "see http://localhost:3000/app.js:4 now";
        let mut screen = TerminalScreen::new(2, 60);
        screen.feed(output.as_bytes());
        let link = screen.link_at(0, 20).unwrap();
        assert_eq!(link.file, None);
        assert_eq!(link.url, "http://localhost:3000/app.js:4");
    }

    #[test]
    fn a_file_reference_that_wraps_is_one_link_on_every_row() {
        let output = "x packages/pkg-name/src/index.ts:40:2";
        let mut screen = TerminalScreen::new(4, 20);
        screen.feed(output.as_bytes());
        let first = screen.link_at(0, 6).unwrap();
        let second = screen.link_at(1, 6).unwrap();
        assert_eq!(first.url, "packages/pkg-name/src/index.ts:40:2");
        assert_eq!(first, second);
    }

    #[test]
    fn an_address_needs_a_host() {
        assert!(address_length(&text("http://")).is_none());
        assert!(address_length(&text("https:///path")).is_none());
        assert!(address_length(&text("http://?q=1")).is_none());
        assert_eq!(address_length(&text("http://a")), Some(8));
    }

    #[test]
    fn control_characters_end_an_address() {
        // The escape sequence that follows is not text on the screen, but a tab is.
        assert_eq!(
            url_in(1, 40, "http://example.com\tnext", 0, 3).as_deref(),
            Some("http://example.com")
        );
    }

    #[test]
    fn a_link_is_found_in_the_history_too() {
        let mut output = String::from("https://example.com/old\r\n");
        for line in 0..30 {
            output.push_str(&format!("line {line}\r\n"));
        }
        let mut screen = TerminalScreen::new(5, 40);
        screen.feed(output.as_bytes());
        // The view is at the bottom, where the address is no longer shown.
        assert_eq!(screen.link_at(0, 0), None);
        screen.scroll(100);
        assert_eq!(
            screen.link_at(0, 5).map(|link| link.url).as_deref(),
            Some("https://example.com/old")
        );
    }
}
