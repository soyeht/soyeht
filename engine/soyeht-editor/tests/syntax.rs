#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_editor::{Error, SyntaxDocument, project, project_bounded};
use soyeht_protocol::{EditorLanguage, HighlightKind, TextEdit, TextPosition, TextRange};

fn edit(line: u32, start: u32, end: u32, text: &str) -> TextEdit {
    TextEdit {
        range: TextRange {
            start: TextPosition {
                line,
                column: start,
            },
            end: TextPosition { line, column: end },
        },
        text: text.to_owned(),
    }
}

#[test]
fn every_closed_language_uses_its_parser_and_plain_text_is_explicit() {
    // The Markdown and shell grammars ship no highlight query; they must still parse.
    let fixtures = [
        (EditorLanguage::Rust, "fn main() {}", true),
        (EditorLanguage::Swift, "func main() {}", true),
        (EditorLanguage::Typescript, "function main() {}", true),
        (EditorLanguage::Python, "def main():\n    pass", true),
        (EditorLanguage::Json, "{\"ok\": true}", true),
        (EditorLanguage::Markdown, "# Title\n", false),
        (EditorLanguage::Shell, "echo ok", false),
    ];
    for (language, source, highlighted) in fixtures {
        let document = SyntaxDocument::new(source, language).unwrap();
        let spans = document.highlights(0, source.lines().count()).unwrap();
        assert_eq!(!spans.is_empty(), highlighted, "{source}");
    }
    let plain = SyntaxDocument::new("any bytes", EditorLanguage::PlainText).unwrap();
    assert!(plain.highlights(0, 1).unwrap().is_empty());
}

#[test]
fn syntax_reuses_the_edited_tree_and_projects_only_requested_lines() {
    let mut document = SyntaxDocument::new(
        "fn main() {\n    let value = 1;\n}\nunchanged\n",
        EditorLanguage::Rust,
    )
    .unwrap();
    document.apply(&edit(1, 16, 17, "\"text\"")).unwrap();
    assert_eq!(document.parsed_revision(), 1);
    assert!(!document.last_changed_ranges().is_empty());
    assert!(
        document
            .last_changed_ranges()
            .iter()
            .all(|range| range.end < document.buffer().byte_len())
    );

    let viewport = project(&document, 0, 2).unwrap();
    assert_eq!(viewport.len(), 2);
    assert_eq!(viewport[0].line, 0);
    assert!(
        viewport[0]
            .highlights
            .iter()
            .any(|span| span.kind == HighlightKind::Keyword)
    );
    assert!(
        viewport[1]
            .highlights
            .iter()
            .any(|span| span.kind == HighlightKind::String)
    );
}

#[test]
fn multi_cursor_edits_update_one_revision_and_reparse_once() {
    let mut document =
        SyntaxDocument::new("let one = 1;\nlet two = 2;", EditorLanguage::Rust).unwrap();
    document
        .apply_all(&[edit(0, 10, 11, "\"first\""), edit(1, 10, 11, "\"second\"")])
        .unwrap();
    assert_eq!(document.buffer().revision(), 1);
    assert_eq!(document.parsed_revision(), 1);
    assert_eq!(
        document.buffer().text(),
        "let one = \"first\";\nlet two = \"second\";"
    );
    assert!(!document.last_changed_ranges().is_empty());
}

#[test]
fn highlight_columns_are_utf16_even_after_non_ascii_text() {
    let document = SyntaxDocument::new("😀 let value = 1;", EditorLanguage::Rust).unwrap();
    let spans = document.highlights(0, 1).unwrap();
    let keyword = spans
        .iter()
        .find(|(_, span)| span.kind == HighlightKind::Keyword)
        .unwrap();
    assert_eq!(keyword.1.start_column, 3);
    assert_eq!(keyword.1.end_column, 6);
}

#[test]
fn viewport_projection_stops_before_the_transport_budget() {
    let document = SyntaxDocument::new("one\ntwo\nthree", EditorLanguage::PlainText).unwrap();
    let lines = project_bounded(&document, 0, 3, 135).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        project_bounded(&document, 0, 1, 10),
        Err(Error::ViewportTooLarge)
    );
}
