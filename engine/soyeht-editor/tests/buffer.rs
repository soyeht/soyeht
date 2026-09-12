#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_editor::{Error, TextBuffer};
use soyeht_protocol::{TextEdit, TextPosition, TextRange};

fn position(line: u32, column: u32) -> TextPosition {
    TextPosition { line, column }
}
fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
    TextEdit {
        range: TextRange {
            start: position(start.0, start.1),
            end: position(end.0, end.1),
        },
        text: text.to_owned(),
    }
}

#[test]
fn piece_table_edits_unicode_and_updates_only_line_boundaries() {
    let mut buffer = TextBuffer::new("alpha\n😀 beta\nomega");
    assert_eq!(buffer.line_count(), 3);
    assert_eq!(buffer.byte_at(&position(1, 2)).unwrap(), 10);
    assert_eq!(buffer.byte_at(&position(1, 1)), Err(Error::InvalidPosition));

    buffer.apply(&edit((1, 3), (1, 7), "B\nC")).unwrap();
    assert_eq!(buffer.text(), "alpha\n😀 B\nC\nomega");
    assert_eq!(buffer.line_count(), 4);
    assert_eq!(buffer.line(1).unwrap(), "😀 B");
    assert_eq!(buffer.line(2).unwrap(), "C");
    assert_eq!(buffer.revision(), 1);
}

#[test]
fn multi_cursor_edits_are_atomic_ordered_and_non_overlapping() {
    let mut buffer = TextBuffer::new("one two three");
    buffer
        .apply_all(&[edit((0, 0), (0, 3), "1"), edit((0, 8), (0, 13), "3")])
        .unwrap();
    assert_eq!(buffer.text(), "1 two 3");
    assert_eq!(buffer.revision(), 1);

    let before = buffer.text();
    let error = buffer.apply_all(&[edit((0, 0), (0, 5), "x"), edit((0, 3), (0, 7), "y")]);
    assert_eq!(error, Err(Error::OverlappingEdits));
    assert_eq!(buffer.text(), before);
}

#[test]
fn a_hundred_thousand_line_buffer_opens_inside_the_contract_budget() {
    let text = "x\n".repeat(100_000);
    let started = std::time::Instant::now();
    let buffer = TextBuffer::new(text);
    assert_eq!(buffer.line_count(), 100_001);
    assert!(started.elapsed() < std::time::Duration::from_millis(150));
}
