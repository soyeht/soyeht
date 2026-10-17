//! A running terminal pane for unit tests that need one without a process behind it.

use crate::{
    core::Core,
    model::{RunningPane, StoredPane},
    pane_observations::SegmentStore,
    screen::Screen,
};
use serde_json::json;
use soyeht_pane_state::{EngineBuild, HarnessIdentity, ObservationPaneKind, RecordedRow};
use soyeht_protocol::Pane;
use std::path::Path;

/// Adds the pane `{label}-pane` to `core`, with its experiment capture `capture`, and returns
/// its id. Its screen is kept under `root`.
pub(crate) fn insert_terminal_pane(
    core: &mut Core,
    root: &Path,
    label: &str,
    capture: bool,
) -> String {
    let pane_id = format!("{label}-pane");
    let pane: Pane = serde_json::from_value(json!({
        "paneId": pane_id, "workspaceId": "workspace", "name": label,
        "cwd": root.display().to_string(), "mode": "terminal", "state": "unknown",
        "source": "none", "confidence": 0.0, "phrase": "Shell active", "generation": 1,
        "cols": 120, "rows": 40, "record": false, "pid": 1,
        "experimentCapture": {"on": capture}, "streamId": format!("{label}-stream"),
    }))
    .unwrap();
    let screen = Screen::open(&root.join("sessions").join(&pane_id), false).unwrap();
    core.panes.insert(
        pane_id.clone(),
        RunningPane {
            stored: StoredPane {
                pane,
                closed: false,
                instance_key: format!("{label}-key"),
                instance_id: Some(format!("{label}-instance")),
                supervisor_id: Some(format!("{label}-supervisor")),
                sequence_base: 0,
            },
            screen,
        },
    );
    pane_id
}

/// Every row the terminal capture of `pane_id` under `root` recorded, across its segments.
pub(crate) fn replay_rows(root: &Path, pane_id: &str, build: EngineBuild) -> Vec<RecordedRow> {
    SegmentStore::open(
        root,
        pane_id,
        ObservationPaneKind::Terminal,
        HarnessIdentity::Unknown,
        build,
    )
    .unwrap()
    .replay()
    .unwrap()
    .into_iter()
    .flat_map(|segment| segment.rows)
    .collect()
}
