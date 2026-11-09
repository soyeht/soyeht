//! The migrations from version 24 on, which `Store::open` applies after the earlier ones.

use super::Result;
use rusqlite::Transaction;

pub(super) fn apply_recent_migrations(tx: &Transaction<'_>, version: u32) -> Result<()> {
    if version < 24 {
        tx.execute_batch(include_str!(
            "../../migrations/024-workspace-created-sequence.sql"
        ))?;
    }
    if version < 25 {
        tx.execute_batch(include_str!("../../migrations/025-agent-message-rate.sql"))?;
    }
    if version < 26 {
        tx.execute_batch(include_str!("../../migrations/026-harness-readiness.sql"))?;
    }
    if version < 27 {
        tx.execute_batch(include_str!(
            "../../migrations/027-pane-created-sequence.sql"
        ))?;
    }
    if version < 28 {
        tx.execute_batch(include_str!("../../migrations/028-push-registrations.sql"))?;
    }
    if version < 29 {
        tx.execute_batch(include_str!("../../migrations/029-editor-tabs.sql"))?;
    }
    if version < 30 {
        tx.execute_batch(include_str!(
            "../../migrations/030-prompt-attachment-files.sql"
        ))?;
        drop_legacy_prompt_attachments(tx)?;
    }
    if version < 31 {
        tx.execute_batch(include_str!("../../migrations/031-harness-sign-in.sql"))?;
        crate::harness_sign_in::adopt_pane_choices(tx)?;
    }
    if version < 32 {
        tx.execute_batch(include_str!("../../migrations/032-usage-opencode.sql"))?;
    }
    if version < 33 {
        tx.execute_batch(include_str!("../../migrations/033-project-trust.sql"))?;
    }
    if version < 34 {
        if lacks_column(tx, "input_intents", "created_unix")? {
            tx.execute_batch(include_str!("../../migrations/034-input-intent-time.sql"))?;
        }
        tx.execute_batch("PRAGMA user_version = 34;")?;
    }
    Ok(())
}

/// Whether `table` exists and has no `column` yet: a migration that adds one runs again on a
/// database whose version was set back (as the tests of earlier migrations do), and SQLite cannot
/// add a column twice.
fn lacks_column(tx: &Transaction<'_>, table: &str, column: &str) -> Result<bool> {
    let (columns, found): (i64, i64) = tx.query_row(
        "SELECT COUNT(*), COALESCE(SUM(name=?2), 0) FROM pragma_table_info(?1)",
        [table, column],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(columns > 0 && found == 0)
}

/// Version 30 names queued attachments by engine-owned paths. A prompt queued
/// with the earlier byte reference cannot be delivered, so it keeps its text.
fn drop_legacy_prompt_attachments(tx: &Transaction<'_>) -> Result<()> {
    for (table, field) in [
        ("agent_prompt_queue", "$.attachments"),
        ("agent_runtime", "$.active.attachments"),
    ] {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )?;
        if exists {
            tx.execute(
                &format!(
                    "UPDATE {table} SET payload=json_remove(payload,?1) \
                     WHERE json_type(payload,?1) IS NOT NULL"
                ),
                [field],
            )?;
        }
    }
    Ok(())
}
