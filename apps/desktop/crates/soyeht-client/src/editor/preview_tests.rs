//! The Markdown preview of a file's page: which files have one, and that it follows the typing.

use super::{EditorFile, page::Page, tests::doc};
use crate::markdown::Refusal;
use serde_json::json;
use soyeht_protocol::limits::EDITOR_VIEWPORT_LINES;

/// A file of `language` with `lines` lines whose page, from `from`, holds `text`.
fn file(language: &str, text: &str, from: u32, lines: u32) -> EditorFile {
    let mut document = doc("f1", "e1", 1, lines, false);
    document["language"] = json!(language);
    let mut file = EditorFile::new(serde_json::from_value(document).unwrap());
    file.page = Some(Page::of_text(from, text));
    file
}

#[test]
fn only_a_markdown_file_with_a_page_has_a_preview() {
    assert!(
        file("rust", "fn main() {}", 0, 1)
            .markdown_preview()
            .is_none()
    );
    let mut unread = file("markdown", "# Title", 0, 1);
    unread.page = None;
    assert!(unread.markdown_preview().is_none());
    let preview = file("markdown", "# Title\ntext", 0, 2)
        .markdown_preview()
        .unwrap()
        .unwrap();
    assert_eq!(preview.source, "# Title\ntext");
    assert!(!preview.is_partial());
}

#[test]
fn the_preview_follows_what_was_typed_and_labels_a_page_that_is_not_the_whole_file() {
    let mut file = file("markdown", "a\nb", 40, 100_000);
    let before = file.markdown_preview().unwrap().unwrap();
    assert!(before.is_partial());
    assert_eq!(before.range_label(), "LINES 41\u{2013}42 OF 100000");
    // A line typed since: the page and the document's count grew together.
    file.page = Some(Page::of_text(40, "a\nb\nc"));
    file.lines += 1;
    let after = file.markdown_preview().unwrap().unwrap();
    assert_eq!(after.source, "a\nb\nc");
    assert_eq!(after.range_label(), "LINES 41\u{2013}43 OF 100001");
}

#[test]
fn a_page_past_the_viewport_limit_is_refused() {
    let text = vec!["x"; EDITOR_VIEWPORT_LINES + 1].join("\n");
    let lines = u32::try_from(EDITOR_VIEWPORT_LINES + 1).unwrap();
    assert_eq!(
        file("markdown", &text, 0, lines).markdown_preview(),
        Some(Err(Refusal::TooManyLines))
    );
}
