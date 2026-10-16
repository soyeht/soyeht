//! What a keystroke changes and what it sends (`EditorEditing` on the Mac; the cases are
//! `contracts/vectors/editor.json`, which both clients pass).
//!
//! A plan holds the replacements to make in the page, where the carets stand afterwards, and the
//! edits the engine is sent. The edits are in the coordinates of the document, from the page's
//! first line, and the engine applies them all to the revision it is told, so a plan with several
//! cursors is one atomic write.

use super::{
    page::{Page, order, ordered, position},
    text::{byte_at, cluster_end, cluster_start, column_at, utf16_len},
};
use soyeht_protocol::{
    TextEdit, TextPosition, TextRange,
    client_policy::{EDITOR_INDENT_OPENERS, EDITOR_INDENT_UNIT, editor_closing},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Replacement {
    pub range: TextRange,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// In document order.
    pub replacements: Vec<Replacement>,
    /// Where each cursor stands once every replacement is made.
    pub carets: Vec<TextPosition>,
    /// What the engine is sent: the replacements that change something.
    pub edits: Vec<TextEdit>,
}

impl Plan {
    /// Makes the replacements in `page`, last first so that the earlier ones keep their places.
    /// False when the page does not hold a range; a plan is made for the page it is applied to,
    /// so that does not happen.
    pub fn apply(&self, page: &mut Page) -> bool {
        self.replacements
            .iter()
            .rev()
            .all(|replacement| page.replace(&replacement.range, &replacement.text))
    }
}

/// A replacement with the number of UTF-16 units from its start that the cursor moves past.
type Change = (Replacement, u32);

/// The range with its start first.
fn forward(range: &TextRange) -> TextRange {
    let (start, end) = ordered(&range.start, &range.end);
    TextRange { start, end }
}

/// The ranges, in document order, when they lie in the page and do not overlap.
fn valid(page: &Page, ranges: &[TextRange]) -> Option<Vec<TextRange>> {
    let mut ordered_ranges: Vec<TextRange> = ranges.iter().map(forward).collect();
    ordered_ranges.sort_by(|a, b| order(&a.start, &b.start));
    let inside = |at: &TextPosition| page.holds(at.line) && at.column <= page.line_end(at.line);
    let mut before: Option<&TextPosition> = None;
    for range in &ordered_ranges {
        if !inside(&range.start) || !inside(&range.end) {
            return None;
        }
        if before.is_some_and(|end| order(&range.start, end).is_lt()) {
            return None;
        }
        before = Some(&range.end);
    }
    (!ordered_ranges.is_empty()).then_some(ordered_ranges)
}

/// Where `at`, which lies after `made` in the document, stands once `made` is made.
fn shifted(at: &TextPosition, made: &Replacement) -> TextPosition {
    let inserted = u32::try_from(made.text.matches('\n').count()).unwrap_or(u32::MAX);
    let removed = made.range.end.line - made.range.start.line;
    let line = at.line + inserted - removed;
    if at.line != made.range.end.line {
        return position(line, at.column);
    }
    let tail = at.column - made.range.end.column;
    let column = match made.text.rsplit_once('\n') {
        Some((_, last)) => utf16_len(last) + tail,
        None => made.range.start.column + utf16_len(&made.text) + tail,
    };
    position(line, column)
}

/// The position `units` UTF-16 units into `inserted`, which starts at `start`.
fn advanced(start: &TextPosition, inserted: &str, units: u32) -> TextPosition {
    let walked = inserted.get(..byte_at(inserted, units)).unwrap_or(inserted);
    match walked.rsplit_once('\n') {
        Some((before, last)) => {
            let lines = u32::try_from(before.matches('\n').count() + 1).unwrap_or(u32::MAX);
            position(start.line + lines, utf16_len(last))
        }
        None => position(start.line, start.column + units.min(utf16_len(inserted))),
    }
}

/// A skipped-over closing character moves the cursor past text that is already there.
fn caret(start: &TextPosition, made: &Replacement, units: u32) -> TextPosition {
    if made.text.is_empty() {
        position(start.line, start.column + units)
    } else {
        advanced(start, &made.text, units)
    }
}

fn plan(changes: Vec<Change>) -> Plan {
    let mut carets = Vec::with_capacity(changes.len());
    for (index, (made, units)) in changes.iter().enumerate() {
        let start = changes[..index]
            .iter()
            .fold(made.range.start.clone(), |at, (earlier, _)| {
                shifted(&at, earlier)
            });
        carets.push(caret(&start, made, *units));
    }
    let edits = changes
        .iter()
        .filter(|(made, _)| {
            order(&made.range.start, &made.range.end).is_ne() || !made.text.is_empty()
        })
        .map(|(made, _)| TextEdit {
            range: made.range.clone(),
            text: made.text.clone(),
        })
        .collect();
    Plan {
        replacements: changes.into_iter().map(|(made, _)| made).collect(),
        carets,
        edits,
    }
}

/// The character at `at`, when the line has one there.
fn character_at(page: &Page, at: &TextPosition) -> Option<String> {
    let text = page.text(at.line);
    let start = byte_at(text, at.column);
    (start < text.len()).then(|| text[start..cluster_end(text, start)].to_owned())
}

/// What a line starts with: its example and tabs.
fn indentation(prefix: &str) -> &str {
    let end = prefix
        .find(|scalar| scalar != ' ' && scalar != '\t')
        .unwrap_or(prefix.len());
    &prefix[..end]
}

/// What typing `inserted` does at each of `ranges`: it replaces what is selected, closes a
/// bracket or quote it opens (and wraps the selection in the pair), steps over the closing
/// character that is already there, and on Return keeps the line's indentation and adds a level
/// after an opening bracket.
pub fn insertion(page: &Page, ranges: &[TextRange], inserted: &str) -> Option<Plan> {
    let closing = editor_closing(inserted);
    let changes = valid(page, ranges)?
        .into_iter()
        .map(|range| {
            let selected = page.between(&range.start, &range.end);
            let at = &range.start;
            let next = character_at(page, at);
            let (text, units) = if let (true, Some(closing), Some(next)) =
                (selected.is_empty(), closing, next.as_deref())
                && next == closing
            {
                (String::new(), 1)
            } else if let Some(closing) = closing {
                let text = format!("{inserted}{selected}{closing}");
                let units = utf16_len(inserted) + utf16_len(&selected);
                (text, units)
            } else if inserted == "\n" {
                let line = page.text(at.line);
                let prefix = line.get(..byte_at(line, at.column)).unwrap_or_default();
                let opens = prefix
                    .trim()
                    .chars()
                    .next_back()
                    .is_some_and(|last| EDITOR_INDENT_OPENERS.contains(last));
                let extra = if opens { EDITOR_INDENT_UNIT } else { "" };
                let text = format!("{}{}{extra}", page.ending(at.line), indentation(prefix));
                let units = utf16_len(&text);
                (text, units)
            } else {
                (inserted.to_owned(), utf16_len(inserted))
            };
            (Replacement { range, text }, units)
        })
        .collect();
    Some(plan(changes))
}

/// The range of the character before `at`, a line break at the start of a line.
fn before(page: &Page, at: &TextPosition) -> Option<TextRange> {
    if at.column > 0 {
        let text = page.text(at.line);
        let end = byte_at(text, at.column);
        let start = cluster_start(text, end);
        return Some(TextRange {
            start: position(at.line, column_at(text, start)),
            end: at.clone(),
        });
    }
    let line = at.line.checked_sub(1).filter(|line| page.holds(*line))?;
    Some(TextRange {
        start: position(line, page.line_end(line)),
        end: at.clone(),
    })
}

/// The range of the character after `at`, a line break at the end of a line.
fn after(page: &Page, at: &TextPosition) -> Option<TextRange> {
    let text = page.text(at.line);
    let start = byte_at(text, at.column);
    if start < text.len() {
        return Some(TextRange {
            start: at.clone(),
            end: position(at.line, column_at(text, cluster_end(text, start))),
        });
    }
    page.holds(at.line + 1).then(|| TextRange {
        start: at.clone(),
        end: position(at.line + 1, 0),
    })
}

/// What Backspace (`backward`) and Delete do at each of `ranges`: a selection goes; at a caret
/// the character before or after it goes, whole, and Backspace between an empty pair removes both.
pub fn deletion(page: &Page, ranges: &[TextRange], backward: bool) -> Option<Plan> {
    let changes = valid(page, ranges)?
        .into_iter()
        .map(|range| {
            let caret = order(&range.start, &range.end).is_eq();
            let range = if !caret {
                range
            } else if backward {
                before(page, &range.start).map_or(range.clone(), |gone| {
                    let pair = page.between(&gone.start, &gone.end);
                    let closes = editor_closing(&pair)
                        .zip(character_at(page, &range.start))
                        .is_some_and(|(closing, next)| closing == next);
                    match after(page, &range.start).filter(|_| closes) {
                        Some(next) => TextRange {
                            start: gone.start,
                            end: next.end,
                        },
                        None => gone,
                    }
                })
            } else {
                after(page, &range.start).unwrap_or(range)
            };
            (
                Replacement {
                    range,
                    text: String::new(),
                },
                0,
            )
        })
        .collect();
    Some(plan(changes))
}

/// What a paste, a cut-and-drop or a replacement does: each range takes its string as it is and
/// a caret follows it. The strings pair with the ranges in the order given.
pub fn verbatim(page: &Page, ranges: &[TextRange], strings: &[String]) -> Option<Plan> {
    if ranges.len() != strings.len() {
        return None;
    }
    valid(page, ranges)?;
    let mut pairs: Vec<(TextRange, &String)> = ranges.iter().map(forward).zip(strings).collect();
    pairs.sort_by(|a, b| order(&a.0.start, &b.0.start));
    Some(plan(
        pairs
            .into_iter()
            .map(|(range, text)| {
                let units = utf16_len(text);
                (
                    Replacement {
                        range,
                        text: text.clone(),
                    },
                    units,
                )
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, vector};
    use serde_json::Value;

    /// The position of UTF-16 `offset` in the page text, whose first line is `from`.
    fn at(text: &str, from: u32, offset: u32) -> TextPosition {
        let mut line = 0;
        let mut start = 0u32;
        for piece in text.split('\n') {
            // The `\r` of a `\r\n` is the line break's, not the line's.
            let end = start + utf16_len(piece.strip_suffix('\r').unwrap_or(piece));
            if offset <= end {
                return position(from + line, offset - start);
            }
            line += 1;
            start = start + utf16_len(piece) + 1;
        }
        position(from + line, 0)
    }

    fn offset_of(page: &Page, at: &TextPosition) -> u32 {
        (page.from_line..at.line)
            .map(|line| {
                page.line_end(line) + u32::from(page.line(line).is_some_and(|l| l.crlf)) + 1
            })
            .sum::<u32>()
            + at.column
    }

    fn text_of(page: &Page) -> String {
        page.lines
            .iter()
            .map(|line| format!("{}{}", line.text, if line.crlf { "\r" } else { "" }))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn numbers(value: &Value) -> (u32, u32) {
        let pair = value.as_array().unwrap();
        (
            u32::try_from(pair[0].as_u64().unwrap()).unwrap(),
            u32::try_from(pair[1].as_u64().unwrap()).unwrap(),
        )
    }

    fn pos(value: &Value) -> TextPosition {
        let (line, column) = numbers(value);
        position(line, column)
    }

    #[test]
    fn a_keystroke_changes_and_sends_what_the_vector_says() {
        for case in cases("editor", "editing") {
            let name = case["name"].as_str().unwrap();
            let text = case["text"].as_str().unwrap();
            let from = u32::try_from(case["viewportStartLine"].as_u64().unwrap()).unwrap();
            let mut page = Page::of_text(from, text);
            let ranges: Vec<TextRange> = case["ranges"]
                .as_array()
                .unwrap()
                .iter()
                .map(|range| {
                    let (location, length) = numbers(range);
                    TextRange {
                        start: at(text, from, location),
                        end: at(text, from, location + length),
                    }
                })
                .collect();
            let made = match case["kind"].as_str().unwrap() {
                "insert" => insertion(&page, &ranges, case["inserted"].as_str().unwrap()),
                "delete" => deletion(&page, &ranges, case["backward"].as_bool().unwrap()),
                _ => {
                    let strings: Vec<String> = case["strings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|string| string.as_str().unwrap().to_owned())
                        .collect();
                    verbatim(&page, &ranges, &strings)
                }
            }
            .unwrap_or_else(|| panic!("{name}: no plan"));
            let edits: Vec<Value> = case["edits"].as_array().unwrap().clone();
            assert_eq!(made.edits.len(), edits.len(), "{name}");
            for (sent, wanted) in made.edits.iter().zip(&edits) {
                assert_eq!(sent.range.start, pos(&wanted["start"]), "{name}");
                assert_eq!(sent.range.end, pos(&wanted["end"]), "{name}");
                assert_eq!(sent.text, wanted["text"].as_str().unwrap(), "{name}");
            }
            assert!(made.apply(&mut page), "{name}");
            assert_eq!(text_of(&page), case["result"].as_str().unwrap(), "{name}");
            let carets: Vec<(u32, u32)> = made
                .carets
                .iter()
                .map(|caret| (offset_of(&page, caret), 0))
                .collect();
            let wanted: Vec<(u32, u32)> = case["selections"]
                .as_array()
                .unwrap()
                .iter()
                .map(numbers)
                .collect();
            assert_eq!(carets, wanted, "{name}");
        }
        assert!(vector("editor")["editing"].as_array().unwrap().len() > 10);
    }

    #[test]
    fn ranges_outside_the_page_or_overlapping_make_no_plan() {
        let page = Page::of_text(3, "abc\ndef");
        let range = |a: (u32, u32), b: (u32, u32)| TextRange {
            start: position(a.0, a.1),
            end: position(b.0, b.1),
        };
        assert!(insertion(&page, &[range((2, 0), (2, 0))], "x").is_none());
        assert!(insertion(&page, &[range((3, 9), (3, 9))], "x").is_none());
        assert!(insertion(&page, &[range((3, 0), (3, 2)), range((3, 1), (3, 3))], "x").is_none());
        assert!(insertion(&page, &[], "x").is_none());
        assert!(verbatim(&page, &[range((3, 0), (3, 0))], &[]).is_none());
        assert!(deletion(&page, &[range((3, 0), (3, 0))], true).is_some());
    }

    #[test]
    fn deleting_forward_at_the_end_of_a_line_joins_the_next_one() {
        let mut page = Page::of_text(0, "ab\ncd");
        let caret = TextRange {
            start: position(0, 2),
            end: position(0, 2),
        };
        let made = deletion(&page, &[caret], false).unwrap();
        assert!(made.apply(&mut page));
        assert_eq!(text_of(&page), "abcd");
        assert_eq!(made.carets, [position(0, 2)]);
    }
}
