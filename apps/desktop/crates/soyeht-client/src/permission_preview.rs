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

/// How many lines a text takes: a text without a line break is one line, and so is an empty one.
pub fn line_count(text: &str) -> usize {
    text.split('\n').count()
}

/// How many lines a well that shows `lines` of text is tall: all of them up to the limit.
pub fn visible_lines(lines: usize) -> usize {
    lines.clamp(1, VISIBLE_LINE_LIMIT)
}

/// The lines of `before` that `after` does not keep, and those of `after` that are new, found
/// from their longest common run of lines.
pub fn preview(change: &PermissionFileChange) -> Preview {
    let before = lines(change.old_text.as_deref().unwrap_or_default());
    let after = lines(&change.new_text);
    if before.len() + after.len() > PERMISSION_PREVIEW_LINES {
        return Preview {
            rows: Vec::new(),
            unavailable: true,
        };
    }
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

    fn change(old: Option<&str>, new: &str) -> PermissionFileChange {
        PermissionFileChange {
            path: "a.rs".into(),
            old_text: old.map(str::to_owned),
            new_text: new.into(),
        }
    }

    fn shown(preview: &Preview) -> Vec<String> {
        preview
            .rows
            .iter()
            .map(|row| match row.kind {
                Kind::Removed => format!("-{}", row.text),
                Kind::Added => format!("+{}", row.text),
            })
            .collect()
    }

    #[test]
    fn a_changed_line_is_removed_then_added_and_the_rest_is_not_shown() {
        let preview = preview(&change(Some("a\nb\nc\n"), "a\nB\nc\n"));
        assert_eq!(shown(&preview), ["-b", "+B"]);
        assert!(!preview.unavailable);
    }

    #[test]
    fn a_new_file_is_all_added_and_a_deleted_one_all_removed() {
        assert_eq!(shown(&preview(&change(None, "x\ny"))), ["+x", "+y"]);
        assert_eq!(shown(&preview(&change(Some("x\ny"), ""))), ["-x", "-y"]);
    }

    #[test]
    fn a_change_over_the_limit_is_not_previewed() {
        let big = "line\n".repeat(PERMISSION_PREVIEW_LINES);
        assert!(preview(&change(Some(&big), "x")).unavailable);
    }

    #[test]
    fn a_well_is_as_tall_as_its_text_up_to_the_limit() {
        assert_eq!(line_count("one"), 1);
        assert_eq!(line_count("a\nb\n"), 3);
        assert_eq!(visible_lines(0), 1);
        assert_eq!(visible_lines(40), VISIBLE_LINE_LIMIT);
    }
}
