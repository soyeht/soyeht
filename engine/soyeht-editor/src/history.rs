//! Bounded undo and redo for one document, independent of any client view.
//!
//! Every write is recorded as a step holding the edits that were sent and the
//! edits that restore the text they replaced. Steps that arrive within
//! [`GROUP_MS`] of each other form one transaction, so a burst of typing undoes
//! as one unit. The history is pinned to the document revision it last saw: any
//! change it did not record (a disk reload, an agent review, a revert) makes the
//! stored positions meaningless, so the history is dropped instead of replayed.

use crate::{AppliedEdit, Result, TextBuffer};
use soyeht_protocol::{TextEdit, TextPosition, TextRange};

/// Writes closer together than this join the previous undo transaction.
pub const GROUP_MS: i64 = 300;
const MAX_TRANSACTIONS: usize = 200;
const MAX_BYTES: usize = 16 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct EditStep {
    forward: Vec<TextEdit>,
    inverse: Vec<TextEdit>,
}

/// A step applied to a buffer, with the cursor placed after its first replacement.
#[derive(Debug)]
pub struct AppliedStep {
    pub step: EditStep,
    pub applied: Vec<AppliedEdit>,
    pub cursor: Option<TextPosition>,
}

impl EditStep {
    /// Applies wire edits to `buffer` and returns the step that can undo and redo them.
    pub fn record(buffer: &mut TextBuffer, edits: &[TextEdit]) -> Result<AppliedStep> {
        apply_tracked(buffer, edits, false)
    }

    fn bytes(&self) -> usize {
        self.forward
            .iter()
            .chain(&self.inverse)
            .map(|edit| edit.text.len())
            .sum()
    }

    fn is_noop(&self) -> bool {
        self.forward
            .iter()
            .all(|edit| edit.text.is_empty() && edit.range.start == edit.range.end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryDirection {
    Undo,
    Redo,
}

/// The result of replaying one transaction: the edit sets in the order they
/// were applied, every applied edit, and where the cursor belongs afterwards.
#[derive(Debug, Default)]
pub struct Replay {
    pub edits: Vec<Vec<TextEdit>>,
    pub applied: Vec<AppliedEdit>,
    pub cursor: Option<TextPosition>,
}

#[derive(Clone, Debug)]
struct Transaction {
    steps: Vec<EditStep>,
    last_ms: Option<i64>,
    bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    revision: Option<u64>,
    bytes: usize,
}

impl EditHistory {
    /// Records a write that moved the document from `base_revision` to `next_revision`.
    pub fn record(&mut self, base_revision: u64, next_revision: u64, step: EditStep, now_ms: i64) {
        if self.revision != Some(base_revision) {
            self.clear();
        }
        self.revision = Some(next_revision);
        if step.is_noop() {
            return;
        }
        self.bytes -= self.redo.drain(..).map(|item| item.bytes).sum::<usize>();
        let bytes = step.bytes();
        self.bytes += bytes;
        match self.undo.last_mut() {
            Some(top)
                if top
                    .last_ms
                    .is_some_and(|last| (0..=GROUP_MS).contains(&(now_ms - last))) =>
            {
                top.steps.push(step);
                top.bytes += bytes;
                top.last_ms = Some(now_ms);
            }
            _ => self.undo.push(Transaction {
                steps: vec![step],
                last_ms: Some(now_ms),
                bytes,
            }),
        }
        while !self.undo.is_empty()
            && (self.undo.len() > MAX_TRANSACTIONS || self.bytes > MAX_BYTES)
        {
            let dropped = self.undo.remove(0);
            self.bytes -= dropped.bytes;
        }
    }

    pub fn can_replay(&self, direction: HistoryDirection, revision: u64) -> bool {
        self.revision == Some(revision) && !self.stack(direction).is_empty()
    }

    /// Replays the newest transaction in `direction` on `buffer`, which must hold
    /// the text at `revision`. The history itself changes only in [`Self::commit`].
    pub fn replay(
        &self,
        direction: HistoryDirection,
        revision: u64,
        buffer: &mut TextBuffer,
    ) -> Result<Option<Replay>> {
        if !self.can_replay(direction, revision) {
            return Ok(None);
        }
        let Some(transaction) = self.stack(direction).last() else {
            return Ok(None);
        };
        let ordered: Vec<&[TextEdit]> = match direction {
            HistoryDirection::Undo => transaction
                .steps
                .iter()
                .rev()
                .map(|step| step.inverse.as_slice())
                .collect(),
            HistoryDirection::Redo => transaction
                .steps
                .iter()
                .map(|step| step.forward.as_slice())
                .collect(),
        };
        let mut replay = Replay::default();
        for edits in ordered {
            let applied = apply_tracked(buffer, edits, true)?;
            replay.edits.push(edits.to_vec());
            replay.applied.extend(applied.applied);
            replay.cursor = applied.cursor.or(replay.cursor);
        }
        Ok(Some(replay))
    }

    /// Moves the replayed transaction to the opposite stack once it is durable.
    pub fn commit(&mut self, direction: HistoryDirection, next_revision: u64) {
        let (from, to) = match direction {
            HistoryDirection::Undo => (&mut self.undo, &mut self.redo),
            HistoryDirection::Redo => (&mut self.redo, &mut self.undo),
        };
        if let Some(mut transaction) = from.pop() {
            transaction.last_ms = None;
            to.push(transaction);
        }
        self.revision = Some(next_revision);
    }

    fn stack(&self, direction: HistoryDirection) -> &[Transaction] {
        match direction {
            HistoryDirection::Undo => &self.undo,
            HistoryDirection::Redo => &self.redo,
        }
    }

    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
}

fn apply_tracked(
    buffer: &mut TextBuffer,
    edits: &[TextEdit],
    restoring: bool,
) -> Result<AppliedStep> {
    let mut spans = edits
        .iter()
        .map(|edit| {
            let (start, end) = buffer.byte_range(&edit.range)?;
            Ok((start, end, edit, buffer.slice(start, end)))
        })
        .collect::<Result<Vec<_>>>()?;
    spans.sort_by_key(|(start, _, _, _)| *start);
    let applied = if restoring {
        buffer.apply_restoring(edits)?
    } else {
        buffer.apply_all(edits)?
    };
    let (mut added, mut removed) = (0, 0);
    let mut inverse = Vec::with_capacity(spans.len());
    let mut cursor = None;
    for (start, end, edit, old) in spans {
        let new_start = start + added - removed;
        let new_end = new_start + edit.text.len();
        let end_position = buffer.position_at_byte(new_end)?;
        cursor.get_or_insert(end_position.clone());
        inverse.push(TextEdit {
            range: TextRange {
                start: buffer.position_at_byte(new_start)?,
                end: end_position,
            },
            text: old,
        });
        added += edit.text.len();
        removed += end - start;
    }
    Ok(AppliedStep {
        step: EditStep {
            forward: edits.to_vec(),
            inverse,
        },
        applied,
        cursor,
    })
}
