//! Find and replace over a whole document, independent of any client page.
//!
//! The client only ever holds one page of a large file, so counting matches,
//! stepping to the next or previous one, and building replacement edits all
//! happen here, against the full text, in one streaming pass each.

use crate::{ContentMatcher, Error, Result};
use memchr::{memchr_iter, memrchr};
use soyeht_protocol::{TextEdit, TextPosition, TextRange};
use std::ops::Range;

/// Replace All folds matches closer than this into one edit, so dense
/// matches cost one buffer splice instead of one per match.
const CLUSTER_GAP_BYTES: usize = 1 << 12;
/// Replace All never sends more edits than this; past it, every match is
/// folded into a single span from the first match to the last.
const MAX_REPLACE_EDITS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindDirection {
    Next,
    Previous,
}

/// One match: its ordinal among every match in the document and its range.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundMatch {
    pub index: usize,
    pub range: TextRange,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindOutcome {
    /// Every match in the document.
    pub total: usize,
    /// The match the search lands on, wrapping around the document ends.
    pub current: Option<FoundMatch>,
    /// Matches inside the requested line window, in order and bounded.
    pub visible: Vec<TextRange>,
}

/// What to search for and from where.
pub struct FindRequest<'a> {
    pub matcher: &'a ContentMatcher,
    pub from: TextPosition,
    pub direction: FindDirection,
    /// Lines whose matches are returned for highlighting.
    pub window: Range<u32>,
    pub visible_limit: usize,
}

/// Counts every match and picks the one after (or before) `from`: the first
/// match starting at or after `from` going forward, or the last match
/// starting before it going backward, wrapping at the document ends.
pub fn find(text: &str, request: &FindRequest<'_>) -> FindOutcome {
    let from = byte_offset(text, &request.from);
    let window = line_window(text, &request.window);
    let mut total = 0;
    let mut first = None;
    let mut last = None;
    let mut chosen = None;
    let mut visible = Vec::new();
    for (start, end) in request.matcher.byte_matches(text) {
        let found = (total, start, end);
        first.get_or_insert(found);
        last = Some(found);
        match request.direction {
            FindDirection::Next if start >= from && chosen.is_none() => chosen = Some(found),
            FindDirection::Previous if start < from => chosen = Some(found),
            _ => {}
        }
        if start >= window.start && end <= window.end && visible.len() < request.visible_limit {
            visible.push((start, end));
        }
        total += 1;
    }
    let landing = chosen.or(match request.direction {
        FindDirection::Next => first,
        FindDirection::Previous => last,
    });
    let mut positions = Positions::new(text);
    let visible = visible
        .into_iter()
        .map(|(start, end)| positions.range(start, end))
        .collect();
    FindOutcome {
        total,
        current: landing.map(|(index, start, end)| FoundMatch {
            index,
            range: Positions::new(text).range(start, end),
        }),
        visible,
    }
}

/// The edit that replaces the match at `range`, or `None` when the text
/// there no longer matches (the document moved under the client).
pub fn replace_one(
    text: &str,
    matcher: &ContentMatcher,
    range: &TextRange,
    replacement: &str,
) -> Option<TextEdit> {
    let start = exact_offset(text, &range.start)?;
    let end = exact_offset(text, &range.end)?;
    matcher.matches_at(text, start, end).then(|| TextEdit {
        range: range.clone(),
        text: matcher.expand(text, start, replacement),
    })
}

/// Edits that replace every match, and how many matches they cover. Nearby
/// matches share an edit and the edit count stays bounded, so a Replace All
/// is one buffer change and one undo step however many matches it touches.
pub fn replace_all(
    text: &str,
    matcher: &ContentMatcher,
    replacement: &str,
) -> Result<(Vec<TextEdit>, usize)> {
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let (mut replaced, mut grown) = (0, 0_usize);
    for (start, end) in matcher.byte_matches(text) {
        let expanded = matcher.expand(text, start, replacement);
        grown = grown.saturating_add(expanded.len().saturating_sub(end - start));
        if text.len().saturating_add(grown) > soyeht_protocol::limits::EDITOR_HISTORY_BYTES {
            return Err(Error::EditTooLarge);
        }
        match spans.last_mut() {
            Some((_, span_end, joined)) if start - *span_end <= CLUSTER_GAP_BYTES => {
                joined.push_str(&text[*span_end..start]);
                joined.push_str(&expanded);
                *span_end = end;
            }
            _ => spans.push((start, end, expanded)),
        }
        replaced += 1;
    }
    if spans.len() > MAX_REPLACE_EDITS {
        spans = vec![fold(text, spans)];
    }
    let mut positions = Positions::new(text);
    let edits = spans
        .into_iter()
        .map(|(start, end, joined)| TextEdit {
            range: positions.range(start, end),
            text: joined,
        })
        .collect();
    Ok((edits, replaced))
}

fn fold(text: &str, spans: Vec<(usize, usize, String)>) -> (usize, usize, String) {
    let mut iterator = spans.into_iter();
    let Some((start, mut end, mut joined)) = iterator.next() else {
        return (0, 0, String::new());
    };
    for (next_start, next_end, next) in iterator {
        joined.push_str(&text[end..next_start]);
        joined.push_str(&next);
        end = next_end;
    }
    (start, end, joined)
}

/// The byte offset of a position, clamped to the document: a line past the
/// end lands at the end, a column past a line's end lands at that end.
fn byte_offset(text: &str, position: &TextPosition) -> usize {
    let Some(line_start) = line_start(text, position.line) else {
        return text.len();
    };
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |offset| line_start + offset);
    let mut units = 0;
    for (offset, character) in text[line_start..line_end].char_indices() {
        if units >= position.column as usize {
            return line_start + offset;
        }
        units += character.len_utf16();
    }
    line_end
}

/// The byte offset of a position that must name a real scalar boundary.
fn exact_offset(text: &str, position: &TextPosition) -> Option<usize> {
    let offset = byte_offset(text, position);
    let line_start = line_start(text, position.line)?;
    let column = text.get(line_start..offset)?.encode_utf16().count();
    (column == position.column as usize).then_some(offset)
}

fn line_start(text: &str, line: u32) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    memchr_iter(b'\n', text.as_bytes())
        .nth(line as usize - 1)
        .map(|newline| newline + 1)
}

fn line_window(text: &str, lines: &Range<u32>) -> Range<usize> {
    if lines.start >= lines.end {
        return 0..0;
    }
    let start = line_start(text, lines.start).unwrap_or(text.len());
    let end = line_start(text, lines.end).unwrap_or(text.len());
    start..end
}

/// Converts ascending byte offsets to positions in one forward pass.
struct Positions<'t> {
    text: &'t str,
    byte: usize,
    line: u32,
    line_start: usize,
}

impl<'t> Positions<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            text,
            byte: 0,
            line: 0,
            line_start: 0,
        }
    }

    fn range(&mut self, start: usize, end: usize) -> TextRange {
        TextRange {
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn position(&mut self, byte: usize) -> TextPosition {
        if byte < self.byte {
            *self = Self::new(self.text);
        }
        let skipped = &self.text.as_bytes()[self.byte..byte];
        let newlines = memchr_iter(b'\n', skipped).count();
        if newlines > 0 {
            self.line += u32::try_from(newlines).unwrap_or(u32::MAX);
            self.line_start = self.byte + memrchr(b'\n', skipped).map_or(0, |index| index + 1);
        }
        self.byte = byte;
        let column = self.text[self.line_start..byte].encode_utf16().count();
        TextPosition {
            line: self.line,
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }
}
