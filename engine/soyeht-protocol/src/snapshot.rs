//! `state.snapshot` answers in pages (`state.snapshot_part` notifications) followed by the
//! result. [`SnapshotAssembler`] is the one place that puts them together and refuses
//! anything incomplete or inconsistent; every client feeds it from its own transport.

use crate::{Error, Id, Result, SnapshotSection, StateSnapshotPartEvent, StateSnapshotResult};
use serde_json::Value;
use std::collections::BTreeMap;

/// The pages of one snapshot, grouped by section, with the result that closed them.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub metadata: StateSnapshotResult,
    sections: BTreeMap<&'static str, Vec<Value>>,
}

impl Snapshot {
    pub fn section(&self, section: &SnapshotSection) -> &[Value] {
        self.sections
            .get(section_name(section))
            .map_or(&[], Vec::as_slice)
    }

    pub fn take(&mut self, section: &SnapshotSection) -> Vec<Value> {
        self.sections
            .remove(section_name(section))
            .unwrap_or_default()
    }
}

/// Collects snapshot pages in order. A page that skips ahead, mixes snapshots, changes the
/// page count or is not a list is an error, and so is a result that does not match the pages.
#[derive(Debug, Default)]
pub struct SnapshotAssembler {
    identity: Option<Id>,
    total: Option<u32>,
    parts: u32,
    sections: BTreeMap<&'static str, Vec<Value>>,
}

impl SnapshotAssembler {
    pub fn push(&mut self, part: StateSnapshotPartEvent) -> Result<()> {
        let consistent = part.index == self.parts
            && part.index < part.of
            && self
                .identity
                .as_ref()
                .is_none_or(|identity| *identity == part.snapshot_id)
            && self.total.is_none_or(|total| total == part.of);
        if !consistent {
            return Err(Error::InconsistentSnapshotPages);
        }
        let Value::Array(items) = part.data else {
            return Err(Error::SnapshotSectionNotArray);
        };
        self.identity = Some(part.snapshot_id);
        self.total = Some(part.of);
        self.parts += 1;
        self.sections
            .entry(section_name(&part.section))
            .or_default()
            .extend(items);
        Ok(())
    }

    pub fn finish(self, metadata: StateSnapshotResult) -> Result<Snapshot> {
        let complete = self.identity.as_ref() == Some(&metadata.snapshot_id)
            && self.total == Some(self.parts)
            && metadata.parts == self.parts;
        if !complete {
            return Err(Error::IncompleteSnapshot);
        }
        Ok(Snapshot {
            metadata,
            sections: self.sections,
        })
    }
}

fn section_name(section: &SnapshotSection) -> &'static str {
    match section {
        SnapshotSection::Workspaces => "workspaces",
        SnapshotSection::Panes => "panes",
        SnapshotSection::Tasks => "tasks",
        SnapshotSection::Loops => "loops",
        SnapshotSection::Requests => "requests",
        SnapshotSection::Attention => "attention",
        SnapshotSection::Devices => "devices",
        SnapshotSection::Capabilities => "capabilities",
        SnapshotSection::Streams => "streams",
        SnapshotSection::Files => "files",
        SnapshotSection::Writing => "writing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn part(id: &str, index: u32, of: u32, section: &str, data: Value) -> StateSnapshotPartEvent {
        serde_json::from_value(json!({
            "snapshotId": id, "index": index, "of": of, "section": section, "data": data
        }))
        .unwrap()
    }

    fn result(id: &str, parts: u32) -> StateSnapshotResult {
        serde_json::from_value(json!({"snapshotId": id, "cursor": "41", "parts": parts})).unwrap()
    }

    #[test]
    fn groups_pages_by_section() {
        let mut assembler = SnapshotAssembler::default();
        assembler
            .push(part("s", 0, 3, "workspaces", json!([{"id": "w1"}])))
            .unwrap();
        assembler
            .push(part("s", 1, 3, "panes", json!([{"id": "p1"}])))
            .unwrap();
        assembler
            .push(part("s", 2, 3, "panes", json!([{"id": "p2"}])))
            .unwrap();
        let mut snapshot = assembler.finish(result("s", 3)).unwrap();
        assert_eq!(snapshot.metadata.cursor, "41");
        assert_eq!(snapshot.section(&SnapshotSection::Workspaces).len(), 1);
        assert_eq!(snapshot.take(&SnapshotSection::Panes).len(), 2);
        assert!(snapshot.section(&SnapshotSection::Panes).is_empty());
        assert!(snapshot.section(&SnapshotSection::Tasks).is_empty());
    }

    #[test]
    fn rejects_skipped_pages_mixed_snapshots_and_changed_totals() {
        let mut assembler = SnapshotAssembler::default();
        assert!(assembler.push(part("s", 1, 2, "panes", json!([]))).is_err());
        let mut assembler = SnapshotAssembler::default();
        assembler.push(part("s", 0, 2, "panes", json!([]))).unwrap();
        assert!(
            assembler
                .push(part("other", 1, 2, "panes", json!([])))
                .is_err()
        );
        assert!(assembler.push(part("s", 1, 3, "panes", json!([]))).is_err());
        let mut assembler = SnapshotAssembler::default();
        assert!(assembler.push(part("s", 0, 0, "panes", json!([]))).is_err());
    }

    #[test]
    fn rejects_non_list_pages() {
        let mut assembler = SnapshotAssembler::default();
        assert!(matches!(
            assembler.push(part("s", 0, 1, "panes", json!({}))),
            Err(Error::SnapshotSectionNotArray)
        ));
    }

    #[test]
    fn rejects_a_result_that_does_not_match_the_pages() {
        let mut assembler = SnapshotAssembler::default();
        assembler.push(part("s", 0, 2, "panes", json!([]))).unwrap();
        assert!(matches!(
            assembler.finish(result("s", 2)),
            Err(Error::IncompleteSnapshot)
        ));
        let mut assembler = SnapshotAssembler::default();
        assembler.push(part("s", 0, 1, "panes", json!([]))).unwrap();
        assert!(assembler.finish(result("other", 1)).is_err());
        assert!(SnapshotAssembler::default().finish(result("s", 0)).is_err());
    }
}
