use crate::{Blame, Diff, Error, Hunk, Line, LineKind, Result};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use std::{collections::BTreeMap, ops::Range, time::Duration};

const CONTEXT_LINES: usize = 3;
const RESULT_BYTES: usize = 768 * 1024;

pub(crate) fn build(current: &str, baseline: &str, blame: Vec<Blame>) -> Result<Diff> {
    let diff = TextDiff::configure()
        .timeout(Duration::from_millis(100))
        .diff_lines(baseline, current);
    let line_map = unchanged_lines(&diff)?;
    let hunks = hunks(&diff)?;
    let blame = remap_blame(blame, &line_map);
    enforce_budget(&hunks, &blame)?;
    Ok(Diff { hunks, blame })
}

pub(crate) fn accept(current: &str, baseline: &str, hunk_id: &str) -> Result<String> {
    resolve(current, baseline, hunk_id, Resolution::Accept)
}

pub(crate) fn reject(current: &str, baseline: &str, hunk_id: &str) -> Result<String> {
    resolve(current, baseline, hunk_id, Resolution::Reject)
}

enum Resolution {
    Accept,
    Reject,
}

fn resolve(current: &str, baseline: &str, hunk_id: &str, choice: Resolution) -> Result<String> {
    let review = build(current, baseline, Vec::new())?;
    let hunk = review
        .hunks
        .iter()
        .find(|hunk| hunk.hunk_id == hunk_id)
        .ok_or(Error::HunkNotFound)?;
    let (target, target_start, target_lines, source, source_start, source_lines) = match choice {
        Resolution::Accept => (
            baseline,
            hunk.old_start,
            hunk.old_lines,
            current,
            hunk.new_start,
            hunk.new_lines,
        ),
        Resolution::Reject => (
            current,
            hunk.new_start,
            hunk.new_lines,
            baseline,
            hunk.old_start,
            hunk.old_lines,
        ),
    };
    let target_range = line_range(target, target_start, target_lines)?;
    let source_range = line_range(source, source_start, source_lines)?;
    let mut resolved = target.to_owned();
    resolved.replace_range(target_range, &source[source_range]);
    Ok(resolved)
}

fn line_range(text: &str, start: u32, count: u32) -> Result<Range<usize>> {
    let mut offsets = vec![0];
    offsets.extend(
        text.match_indices('\n')
            .map(|(index, _)| index.saturating_add(1)),
    );
    if offsets.last().copied() != Some(text.len()) {
        offsets.push(text.len());
    }
    let start = usize::try_from(start).map_err(|_| Error::InvalidHunkRange)?;
    let end = start
        .checked_add(usize::try_from(count).map_err(|_| Error::InvalidHunkRange)?)
        .ok_or(Error::InvalidHunkRange)?;
    let from = offsets.get(start).copied().ok_or(Error::InvalidHunkRange)?;
    let to = offsets.get(end).copied().ok_or(Error::InvalidHunkRange)?;
    Ok(from..to)
}

fn unchanged_lines(diff: &TextDiff<'_, '_, str>) -> Result<BTreeMap<u32, u32>> {
    let mut lines = BTreeMap::new();
    for change in diff.iter_all_changes() {
        if change.tag() != ChangeTag::Equal {
            continue;
        }
        let old = index(change.old_index())?;
        let new = index(change.new_index())?;
        lines.insert(old, new);
    }
    Ok(lines)
}

fn hunks(diff: &TextDiff<'_, '_, str>) -> Result<Vec<Hunk>> {
    diff.grouped_ops(CONTEXT_LINES)
        .into_iter()
        .map(|ops| {
            let old = combined_range(ops.iter().map(similar::DiffOp::old_range));
            let new = combined_range(ops.iter().map(similar::DiffOp::new_range));
            let mut lines = Vec::new();
            for operation in &ops {
                for change in diff.iter_changes(operation) {
                    lines.push(line(
                        change.tag(),
                        change.old_index(),
                        change.new_index(),
                        change.value(),
                    )?);
                }
            }
            let old_start = number(old.start)?;
            let old_lines = number(old.len())?;
            let new_start = number(new.start)?;
            let new_lines = number(new.len())?;
            let hunk_id = hunk_id(old_start, old_lines, new_start, new_lines, &lines);
            Ok(Hunk {
                hunk_id,
                old_start,
                old_lines,
                new_start,
                new_lines,
                lines,
            })
        })
        .collect()
}

fn line(tag: ChangeTag, old: Option<usize>, new: Option<usize>, text: &str) -> Result<Line> {
    let kind = match tag {
        ChangeTag::Equal => LineKind::Context,
        ChangeTag::Delete => LineKind::Removed,
        ChangeTag::Insert => LineKind::Added,
    };
    Ok(Line {
        kind,
        old_line: optional_index(old)?,
        new_line: optional_index(new)?,
        text: without_line_ending(text).to_owned(),
    })
}

fn combined_range(mut ranges: impl Iterator<Item = Range<usize>>) -> Range<usize> {
    let Some(first) = ranges.next() else {
        return 0..0;
    };
    ranges.fold(first, |joined, item| joined.start..item.end)
}

fn without_line_ending(text: &str) -> &str {
    text.strip_suffix("\n")
        .and_then(|value| value.strip_suffix("\r").or(Some(value)))
        .unwrap_or(text)
}

fn hunk_id(
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    lines: &[Line],
) -> String {
    let mut hash = Sha256::new();
    hash.update(old_start.to_be_bytes());
    hash.update(old_lines.to_be_bytes());
    hash.update(new_start.to_be_bytes());
    hash.update(new_lines.to_be_bytes());
    for line in lines {
        hash.update([line.kind as u8]);
        hash.update(line.text.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

fn remap_blame(blame: Vec<Blame>, lines: &BTreeMap<u32, u32>) -> Vec<Blame> {
    blame
        .into_iter()
        .filter_map(|mut item| {
            let current = lines.get(&item.line)?;
            item.line = *current;
            Some(item)
        })
        .collect()
}

fn enforce_budget(hunks: &[Hunk], blame: &[Blame]) -> Result<()> {
    let line_bytes = hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .try_fold(0_usize, |sum, line| {
            sum.checked_add(line.text.len() + 96)
                .ok_or(Error::OutputTooLarge)
        })?;
    let total = blame.iter().try_fold(line_bytes, |sum, item| {
        sum.checked_add(item.commit.len() + item.author.len() + item.summary.len() + 96)
            .ok_or(Error::OutputTooLarge)
    })?;
    (total <= RESULT_BYTES)
        .then_some(())
        .ok_or(Error::OutputTooLarge)
}

fn optional_index(value: Option<usize>) -> Result<Option<u32>> {
    value.map(number).transpose()
}

fn index(value: Option<usize>) -> Result<u32> {
    number(value.ok_or(Error::NumericOverflow)?)
}

fn number(value: usize) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::NumericOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_projects_context_and_remaps_only_unchanged_blame() {
        let baseline = "zero\none\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n";
        let current = "zero\none changed\ntwo\nthree\nfour\nfive\nsix\ninserted\nseven\neight\n";
        let blame = (0..9)
            .map(|line| Blame {
                line,
                commit: "a".repeat(40),
                author: "writer".into(),
                summary: format!("line {line}"),
            })
            .collect();
        let result = build(current, baseline, blame).unwrap();
        assert_eq!(result.hunks.len(), 1);
        assert_eq!(result.hunks[0].old_start, 0);
        assert_eq!(result.hunks[0].new_start, 0);
        assert!(
            result.hunks[0]
                .lines
                .iter()
                .any(|line| line.kind == LineKind::Removed)
        );
        assert!(
            result.hunks[0]
                .lines
                .iter()
                .any(|line| line.kind == LineKind::Added)
        );
        assert_eq!(
            result
                .blame
                .iter()
                .map(|item| item.line)
                .collect::<Vec<_>>(),
            vec![0, 2, 3, 4, 5, 6, 8, 9]
        );
    }

    #[test]
    fn hunk_identity_is_stable_and_changes_with_content() {
        let first = build("one\ntwo\n", "one\n", vec![]).unwrap();
        let repeated = build("one\ntwo\n", "one\n", vec![]).unwrap();
        let changed = build("one\nthree\n", "one\n", vec![]).unwrap();
        assert_eq!(first.hunks[0].hunk_id, repeated.hunks[0].hunk_id);
        assert_ne!(first.hunks[0].hunk_id, changed.hunks[0].hunk_id);
    }

    #[test]
    fn bounded_result_fails_instead_of_returning_an_incomplete_diff() {
        let current = format!("{}\n", "x".repeat(RESULT_BYTES));
        assert_eq!(build(&current, "", vec![]), Err(Error::OutputTooLarge));
    }

    #[test]
    fn one_hunk_can_be_accepted_or_rejected_without_touching_the_other() {
        let baseline =
            "zero\none\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";
        let current =
            "zero\nONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\neleven\ntwelve\n";
        let review = build(current, baseline, vec![]).unwrap();
        assert_eq!(review.hunks.len(), 2);
        let first = &review.hunks[0].hunk_id;

        let accepted_base = accept(current, baseline, first).unwrap();
        assert!(accepted_base.contains("ONE"));
        assert!(accepted_base.contains("ten"));
        let accepted_pending = build(current, &accepted_base, vec![]).unwrap();
        assert_eq!(accepted_pending.hunks.len(), 1);
        assert!(
            accepted_pending.hunks[0]
                .lines
                .iter()
                .any(|line| line.text == "TEN")
        );

        let rejected_current = reject(current, baseline, first).unwrap();
        assert!(rejected_current.contains("one"));
        assert!(rejected_current.contains("TEN"));
        let rejected_pending = build(&rejected_current, baseline, vec![]).unwrap();
        assert_eq!(rejected_pending.hunks.len(), 1);
        assert!(
            rejected_pending.hunks[0]
                .lines
                .iter()
                .any(|line| line.text == "TEN")
        );
    }

    #[test]
    fn insertion_deletion_eof_and_missing_hunks_are_resolved_fail_closed() {
        let inserted = build("one\ntwo", "one\n", vec![]).unwrap();
        assert_eq!(
            reject("one\ntwo", "one\n", &inserted.hunks[0].hunk_id).unwrap(),
            "one\n"
        );
        let deleted = build("one\n", "one\ntwo", vec![]).unwrap();
        assert_eq!(
            reject("one\n", "one\ntwo", &deleted.hunks[0].hunk_id).unwrap(),
            "one\ntwo"
        );
        assert_eq!(reject("one", "two", "missing"), Err(Error::HunkNotFound));
    }
}
