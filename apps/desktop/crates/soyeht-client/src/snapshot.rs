//! The typed engine state a fresh connection starts from. The pages themselves are put
//! together by the shared protocol crate; this reads the sections the desktop client uses.

use crate::{Error, Result};
use serde_json::Value;
use soyeht_protocol::{ClientFocus, Pane, SnapshotSection, Workspace, snapshot};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub workspaces: Vec<Workspace>,
    pub panes: Vec<Pane>,
    pub focus: Option<ClientFocus>,
    /// The requests agents are waiting on (permissions), as the engine lists them.
    pub requests: Vec<Value>,
    /// The event cursor to subscribe from.
    pub cursor: u64,
}

impl Snapshot {
    pub fn from_pages(mut pages: snapshot::Snapshot) -> Result<Self> {
        let cursor = pages
            .metadata
            .cursor
            .parse()
            .map_err(|_| Error::Invalid("snapshot cursor is not a decimal"))?;
        Ok(Self {
            workspaces: decode(pages.take(&SnapshotSection::Workspaces))?,
            panes: decode(pages.take(&SnapshotSection::Panes))?,
            requests: pages.take(&SnapshotSection::Requests),
            focus: pages.metadata.focus,
            cursor,
        })
    }
}

fn decode<T: serde::de::DeserializeOwned>(items: Vec<Value>) -> Result<Vec<T>> {
    items
        .into_iter()
        .map(|item| serde_json::from_value(item).map_err(Error::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{pane_json, workspace_json};
    use serde_json::json;
    use soyeht_protocol::snapshot::SnapshotAssembler;

    fn pages(cursor: &str) -> snapshot::Snapshot {
        let mut assembler = SnapshotAssembler::default();
        for (index, (section, data)) in [
            ("workspaces", json!([workspace_json("w1")])),
            ("panes", json!([pane_json("p1", "w1", "terminal")])),
        ]
        .into_iter()
        .enumerate()
        {
            let part = serde_json::from_value(json!({
                "snapshotId": "s", "index": index, "of": 2, "section": section, "data": data
            }))
            .unwrap();
            assembler.push(part).unwrap();
        }
        let result = serde_json::from_value(json!({
            "snapshotId": "s", "cursor": cursor, "parts": 2
        }))
        .unwrap();
        assembler.finish(result).unwrap()
    }

    #[test]
    fn typed_state_comes_from_the_workspace_and_pane_sections() {
        let snapshot = Snapshot::from_pages(pages("41")).unwrap();
        assert_eq!(snapshot.cursor, 41);
        assert_eq!(snapshot.workspaces[0].workspace_id, "w1");
        assert_eq!(snapshot.panes[0].pane_id, "p1");
    }

    #[test]
    fn a_cursor_that_is_not_a_decimal_is_refused() {
        assert!(Snapshot::from_pages(pages("x")).is_err());
    }
}
