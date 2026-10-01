#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use soyeht_editor::{EditHistory, EditStep, GROUP_MS, HistoryDirection, TextBuffer};
use soyeht_protocol::{TextEdit, TextPosition};
use support::{edit, position};

/// One open document as the engine holds it: text, revision and history.
struct Doc {
    buffer: TextBuffer,
    history: EditHistory,
    revision: u64,
}

impl Doc {
    fn new(text: &str) -> Self {
        Self {
            buffer: TextBuffer::new(text),
            history: EditHistory::default(),
            revision: 1,
        }
    }

    fn write(&mut self, edits: &[TextEdit], now_ms: i64) {
        let step = EditStep::record(&mut self.buffer, edits).unwrap().step;
        self.history
            .record(self.revision, self.revision + 1, step, now_ms);
        self.revision += 1;
    }

    fn replay(&mut self, direction: HistoryDirection) -> Option<TextPosition> {
        let replay = self
            .history
            .replay(direction, self.revision, &mut self.buffer)
            .unwrap()?;
        self.history.commit(direction, self.revision + 1);
        self.revision += 1;
        replay.cursor
    }

    fn can(&self, direction: HistoryDirection) -> bool {
        self.history.can_replay(direction, self.revision)
    }

    fn text(&self) -> String {
        self.buffer.text()
    }
}

#[test]
fn a_typing_burst_undoes_as_one_unit_and_redoes_back() {
    let mut doc = Doc::new("fn main() {}\n");
    for (index, character) in ["l", "e", "t"].into_iter().enumerate() {
        let column = 11 + index as u32;
        doc.write(
            &[edit((0, column), (0, column), character)],
            1_000 + index as i64 * 50,
        );
    }
    assert_eq!(doc.text(), "fn main() {let}\n");

    assert_eq!(doc.replay(HistoryDirection::Undo), Some(position(0, 11)));
    assert_eq!(doc.text(), "fn main() {}\n");
    assert!(!doc.can(HistoryDirection::Undo));

    assert_eq!(doc.replay(HistoryDirection::Redo), Some(position(0, 14)));
    assert_eq!(doc.text(), "fn main() {let}\n");
    assert!(!doc.can(HistoryDirection::Redo));
}

#[test]
fn a_pause_starts_a_new_transaction_and_new_edits_drop_redo() {
    let mut doc = Doc::new("ab");
    doc.write(&[edit((0, 2), (0, 2), "c")], 0);
    doc.write(&[edit((0, 3), (0, 3), "d")], GROUP_MS + 1);
    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), "abc");
    assert!(doc.can(HistoryDirection::Redo));

    doc.write(&[edit((0, 0), (0, 0), "x")], GROUP_MS + 2);
    assert!(!doc.can(HistoryDirection::Redo));
    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), "abc");
    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), "ab");
}

#[test]
fn multi_cursor_unicode_and_multiline_edits_restore_exactly() {
    let original = "😀 one\ntwo three\nfour";
    let mut doc = Doc::new(original);
    doc.write(
        &[
            edit((0, 3), (1, 3), "ONE\nTWO\nTWO"),
            edit((1, 4), (1, 9), ""),
            edit((2, 0), (2, 4), "4"),
        ],
        0,
    );
    assert_eq!(doc.text(), "😀 ONE\nTWO\nTWO \n4");
    doc.write(&[edit((0, 0), (0, 2), "")], 10);
    assert_eq!(doc.text(), " ONE\nTWO\nTWO \n4");

    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), original);
    doc.replay(HistoryDirection::Redo);
    assert_eq!(doc.text(), " ONE\nTWO\nTWO \n4");
}

#[test]
fn an_unrecorded_change_invalidates_the_history_instead_of_replaying_stale_positions() {
    let mut doc = Doc::new("alpha");
    doc.write(&[edit((0, 5), (0, 5), "!")], 0);
    assert!(doc.can(HistoryDirection::Undo));

    // A disk reload or agent review moves the revision without a recorded step.
    doc.revision += 1;
    doc.buffer = TextBuffer::new("something else entirely");
    assert_eq!(doc.replay(HistoryDirection::Undo), None);
    assert_eq!(doc.text(), "something else entirely");

    doc.write(&[edit((0, 0), (0, 0), ">")], 10);
    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), "something else entirely");
    assert!(!doc.can(HistoryDirection::Undo));
}

#[test]
fn no_op_writes_keep_the_history_pinned_to_the_new_revision() {
    let mut doc = Doc::new("x");
    doc.write(&[edit((0, 1), (0, 1), "y")], 0);
    doc.write(&[edit((0, 0), (0, 0), "")], 5_000);
    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), "x");
}

#[test]
fn restoring_a_deletion_larger_than_one_wire_edit_is_allowed() {
    let large = "z".repeat(soyeht_protocol::limits::EDITOR_EDIT_BYTES + 1);
    let mut doc = Doc::new(&large);
    let end = u32::try_from(large.len()).unwrap();
    doc.write(&[edit((0, 0), (0, end), "")], 0);
    assert_eq!(doc.text(), "");
    doc.replay(HistoryDirection::Undo);
    assert_eq!(doc.text(), large);
}
