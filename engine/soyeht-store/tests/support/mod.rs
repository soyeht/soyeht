//! Fixtures shared by the store's integration tests.
#![allow(dead_code)] // each test binary uses a different part of this module

use rusqlite::Connection;
use serde_json::json;
use soyeht_protocol::{Pane, PaneMode, PaneState, StateSource};
use soyeht_store::database::{Change, Result, Store};
use std::{fs, path::PathBuf};
use uuid::Uuid;

/// An empty profile directory in the system's temporary folder, removed when it drops.
pub struct Profile(pub PathBuf);

impl Profile {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!("soyeht-next-store-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub fn store(&self) -> Store {
        Store::open(&self.0).unwrap()
    }

    /// The profile's database file opened without the store, to build or inspect an old schema.
    pub fn connection(&self) -> Connection {
        Connection::open(self.0.join("soyeht.db")).unwrap()
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The first `count` migrations, in order: the schema a database had at that version.
const MIGRATIONS: [&str; 20] = [
    include_str!("../../migrations/001-initial.sql"),
    include_str!("../../migrations/002-client-state.sql"),
    include_str!("../../migrations/003-english-values.sql"),
    include_str!("../../migrations/004-agent-sessions.sql"),
    include_str!("../../migrations/005-native-replay.sql"),
    include_str!("../../migrations/006-agent-auth-snapshot.sql"),
    include_str!("../../migrations/007-workspace-tasks.sql"),
    include_str!("../../migrations/008-task-reviews.sql"),
    include_str!("../../migrations/009-task-block-retries.sql"),
    include_str!("../../migrations/010-agent-pane-role.sql"),
    include_str!("../../migrations/011-usage-readings.sql"),
    include_str!("../../migrations/012-editor-files.sql"),
    include_str!("../../migrations/013-editor-project-replacements.sql"),
    include_str!("../../migrations/014-language-servers.sql"),
    include_str!("../../migrations/015-editor-agent-reviews.sql"),
    include_str!("../../migrations/016-web-session-grants.sql"),
    include_str!("../../migrations/017-web-permissions.sql"),
    include_str!("../../migrations/018-web-permission-operations.sql"),
    include_str!("../../migrations/019-agent-prompt-attachments.sql"),
    include_str!("../../migrations/020-writing-documents.sql"),
];

/// Applies the first `count` migrations to `connection`.
pub fn apply_migrations(connection: &Connection, count: usize) {
    for migration in &MIGRATIONS[..count] {
        connection.execute_batch(migration).unwrap();
    }
}

/// Commits `apply` as one mutation of scope `test` and idempotency key `key`, with no result
/// and no event.
pub fn commit<T>(store: &mut Store, key: &str, apply: impl FnOnce(&Change<'_>) -> Result<T>) {
    store
        .mutate("test", key, key, |change| {
            apply(change)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

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
