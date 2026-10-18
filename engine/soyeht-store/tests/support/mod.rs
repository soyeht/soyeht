//! Fixtures shared by the store's integration tests.

use soyeht_protocol::{Pane, PaneMode, PaneState, StateSource};

/// An idle pane with no optional field set: a test overrides what it exercises with
/// `Pane { name: .., ..pane(..) }`.
pub fn pane(id: &str, workspace_id: &str, mode: PaneMode, cwd: &str) -> Pane {
    Pane {
        pane_id: id.into(),
        workspace_id: workspace_id.into(),
        name: id.into(),
        cwd: cwd.into(),
        mode,
        state: PaneState::Idle,
        source: StateSource::Protocol,
        confidence: 1.0,
        phrase: String::new(),
        generation: 1,
        cols: 80,
        rows: 24,
        record: false,
        experiment_capture: None,
        pid: None,
        exit_code: None,
        stream_id: None,
        agent: None,
        url: None,
        title: None,
        viewport: None,
        incognito: None,
        web_session: None,
        writing_kind: None,
        writing_section: None,
    }
}
