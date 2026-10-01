//! Position and edit builders shared by the editor integration tests.

use soyeht_protocol::{TextEdit, TextPosition, TextRange};

pub fn position(line: u32, column: u32) -> TextPosition {
    TextPosition { line, column }
}

pub fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
    TextEdit {
        range: TextRange {
            start: position(start.0, start.1),
            end: position(end.0, end.1),
        },
        text: text.to_owned(),
    }
}
