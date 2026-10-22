//! What the editor's Markdown preview shows of the page the editor holds, as the Mac's
//! `MarkdownViewportPreview` does (the cases are `contracts/vectors/editor.json`,
//! `markdownPreview`, which both clients pass).
//!
//! The preview is bounded like the page: past the viewport limit in lines, or the message size in
//! bytes, it is refused instead of parsed. A page that is not the whole file is partial, and the
//! preview says which lines it shows. The parse itself is the replies' (`markdown::parse`).

use soyeht_protocol::limits::{EDITOR_VIEWPORT_LINES, JSON_BYTES};

/// Why a page is not previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    TooManyLines,
    TooManyBytes,
}

/// The Markdown source of a page and which lines of the file it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The lines of the page joined by a newline.
    pub source: String,
    from_line: u32,
    to_line: u32,
    total_lines: u32,
}

impl Preview {
    /// The preview of `lines`, the page of a file of `total_lines` lines that starts at
    /// `from_line`.
    pub fn of_lines<'a>(
        lines: impl ExactSizeIterator<Item = &'a str>,
        from_line: u32,
        total_lines: u32,
    ) -> Result<Self, Refusal> {
        let count = lines.len();
        if count > EDITOR_VIEWPORT_LINES {
            return Err(Refusal::TooManyLines);
        }
        let mut source = String::new();
        for (index, line) in lines.enumerate() {
            if index > 0 {
                source.push('\n');
            }
            source.push_str(line);
            if source.len() > JSON_BYTES {
                return Err(Refusal::TooManyBytes);
            }
        }
        Ok(Self {
            source,
            from_line,
            to_line: from_line.saturating_add(u32::try_from(count).unwrap_or(u32::MAX)),
            total_lines,
        })
    }

    /// The page is not the whole file.
    pub fn is_partial(&self) -> bool {
        self.from_line > 0 || self.to_line < self.total_lines
    }

    /// The lines the page holds, one-based: `LINES 41\u{2013}43 OF 100000`.
    pub fn range_label(&self) -> String {
        format!(
            "LINES {}\u{2013}{} OF {}",
            u64::from(self.from_line) + 1,
            self.to_line,
            self.total_lines
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::vector;

    /// The lines of a vector case: listed, or generated as `lines` of `bytesPerLine` bytes.
    fn lines_of(case: &serde_json::Value) -> Vec<String> {
        if let Some(listed) = case["lines"].as_array() {
            return listed
                .iter()
                .map(|line| line.as_str().unwrap().to_owned())
                .collect();
        }
        let generated = &case["generated"];
        let count = generated["lines"].as_u64().unwrap() as usize;
        let bytes = generated["bytesPerLine"].as_u64().unwrap() as usize;
        vec!["x".repeat(bytes); count]
    }

    #[test]
    fn the_preview_follows_the_vector() {
        for case in vector("editor")["markdownPreview"]["cases"]
            .as_array()
            .unwrap()
        {
            let lines = lines_of(case);
            let from = u32::try_from(case["fromLine"].as_u64().unwrap()).unwrap();
            let total = u32::try_from(case["totalLines"].as_u64().unwrap()).unwrap();
            let made = Preview::of_lines(lines.iter().map(String::as_str), from, total);
            let name = &case["name"];
            match case["refusal"].as_str() {
                Some("tooManyLines") => assert_eq!(made, Err(Refusal::TooManyLines), "{name}"),
                Some("tooManyBytes") => assert_eq!(made, Err(Refusal::TooManyBytes), "{name}"),
                _ => {
                    let preview = made.unwrap();
                    assert_eq!(preview.source, lines.join("\n"), "{name}");
                    assert_eq!(preview.is_partial(), case["isPartial"], "{name}");
                    assert_eq!(preview.to_line, case["toLine"], "{name}");
                    assert_eq!(
                        preview.range_label(),
                        case["label"].as_str().unwrap(),
                        "{name}"
                    );
                }
            }
        }
    }
}
