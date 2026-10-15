//! What the person sees of a file change before approving it, the rule of the Mac's
//! `PermissionPreviewPolicy`: the lines that went away, then the lines that came in, or a note
//! that the change is too large to show.

use soyeht_protocol::{PermissionFileChange, limits::PERMISSION_PREVIEW_LINES};

/// Lines a permission well shows before it scrolls.
pub const VISIBLE_LINE_LIMIT: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub rows: Vec<Row>,
    /// The change has more lines than a preview carries; the person is told to look at the file.
    pub unavailable: bool,
}

/// The lines of a text: a final line break ends the last line instead of starting another.
fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// How many lines a text takes, as the Mac counts them (`Character.isNewline`): a text without a
/// line break is one line, and so is an empty one. Besides `\n`, a lone `\r`, a vertical tab, a
/// form feed, NEL and the Unicode line and paragraph separators end a line; `\r\n` ends one.
pub fn line_count(text: &str) -> usize {
    let mut count = 1;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                chars.next_if_eq(&'\n');
                count += 1;
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => count += 1,
            _ => {}
        }
    }
    count
}

/// How many lines a well that shows `lines` of text is tall: all of them up to the limit.
pub fn visible_lines(lines: usize) -> usize {
    lines.clamp(1, VISIBLE_LINE_LIMIT)
}

/// The lines of `before` that `after` does not keep, and those of `after` that are new. The lines
/// both start or end with are kept; of the rest, a longest common run of lines is kept, and when
/// several are as long, the one found by taking the earliest line of `before` out first. The Mac
/// follows the same rule (`PermissionPreviewPolicy`), because the standard library's diff does not
/// promise which of several equally short differences it returns.
pub fn preview(change: &PermissionFileChange) -> Preview {
    let before = lines(change.old_text.as_deref().unwrap_or_default());
    let after = lines(&change.new_text);
    if before.len() + after.len() > PERMISSION_PREVIEW_LINES {
        return Preview {
            rows: Vec::new(),
            unavailable: true,
        };
    }
    let head = before
        .iter()
        .zip(&after)
        .take_while(|(old, new)| old == new)
        .count();
    let tail = before[head..]
        .iter()
        .rev()
        .zip(after[head..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    let before = &before[head..before.len() - tail];
    let after = &after[head..after.len() - tail];
    // `kept[i][j]`: how many lines the tails `before[i..]` and `after[j..]` share in order.
    let mut kept = vec![vec![0usize; after.len() + 1]; before.len() + 1];
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            kept[i][j] = if before[i] == after[j] {
                kept[i + 1][j + 1] + 1
            } else {
                kept[i + 1][j].max(kept[i][j + 1])
            };
        }
    }
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < before.len() && j < after.len() {
        if before[i] == after[j] {
            i += 1;
            j += 1;
        } else if kept[i + 1][j] >= kept[i][j + 1] {
            removed.push(before[i]);
            i += 1;
        } else {
            added.push(after[j]);
            j += 1;
        }
    }
    removed.extend(&before[i..]);
    added.extend(&after[j..]);
    let row = |kind| {
        move |text: &str| Row {
            kind,
            text: text.to_owned(),
        }
    };
    Preview {
        rows: removed
            .into_iter()
            .map(row(Kind::Removed))
            .chain(added.into_iter().map(row(Kind::Added)))
            .collect(),
        unavailable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cases, vector};

    fn change(case: &serde_json::Value) -> PermissionFileChange {
        let new_text = match case["newLineCount"].as_u64() {
            Some(count) => "line\n".repeat(count as usize),
            None => case["newText"].as_str().unwrap().to_owned(),
        };
        PermissionFileChange {
            path: "a.rs".into(),
            old_text: case["oldText"].as_str().map(str::to_owned),
            new_text,
        }
    }

    #[test]
    fn previews_are_the_cases_the_mac_passes() {
        for case in cases("permission-preview", "previews") {
            let preview = preview(&change(&case));
            let name = case["name"].as_str().unwrap();
            assert_eq!(
                preview.unavailable,
                case["unavailable"].as_bool().unwrap_or(false),
                "{name}"
            );
            if let Some(expected) = case["rows"].as_array() {
                let shown: Vec<(String, String)> = preview
                    .rows
                    .iter()
                    .map(|row| {
                        let sign = if row.kind == Kind::Removed { "-" } else { "+" };
                        (sign.to_owned(), row.text.clone())
                    })
                    .collect();
                let expected: Vec<(String, String)> = expected
                    .iter()
                    .map(|row| {
                        (
                            row[0].as_str().unwrap().to_owned(),
                            row[1].as_str().unwrap().to_owned(),
                        )
                    })
                    .collect();
                assert_eq!(shown, expected, "{name}");
            }
        }
    }

    #[test]
    fn lines_are_counted_as_the_mac_counts_them() {
        for case in cases("permission-preview", "lineCounts") {
            assert_eq!(
                line_count(case["text"].as_str().unwrap()) as u64,
                case["lines"].as_u64().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn a_well_is_as_tall_as_its_text_up_to_the_limit() {
        assert_eq!(
            VISIBLE_LINE_LIMIT as u64,
            vector("permission-preview")["visibleLineLimit"]
        );
        for case in cases("permission-preview", "visibleLines") {
            assert_eq!(
                visible_lines(case["lines"].as_u64().unwrap() as usize) as u64,
                case["height"].as_u64().unwrap(),
                "{case}"
            );
        }
    }
}
