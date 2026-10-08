//! The task questions the engine asks on every tick: what is due, what is
//! unresolved. Each answers from the rows and the time alone, so an empty
//! answer rests until a write or the next due time (`resting`).

use crate::{
    database::{Result, Store},
    tasks::{
        ReviewStage, TaskReviewRecord, UnresolvedTaskReview, decoded, decoded_review_stage,
        review_stage_rank,
    },
};
use serde::de::DeserializeOwned;
use soyeht_protocol::{LoopConfig, Task};

impl Store {
    pub fn due_loop_configs(&self, now_ms: u64) -> Result<Vec<(String, LoopConfig, u64)>> {
        let now = now_ms.min(i64::MAX as u64) as i64;
        self.rested_rows(
            "task_loops",
            now,
            || {
                let mut statement = self.connection.prepare_cached(
                    "SELECT pane_id,payload,next_tick_ms FROM task_loops WHERE next_tick_ms<=?1 ORDER BY next_tick_ms,pane_id",
                )?;
                let rows = statement.query_map([now], |row| {
                    let payload: String = row.get(1)?;
                    Ok((
                        row.get(0)?,
                        serde_json::from_str::<LoopConfig>(&payload).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                payload.len(),
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?,
                        row.get::<_, i64>(2)?.max(0) as u64,
                    ))
                })?;
                rows.filter(|row| {
                    row.as_ref()
                        .map(|(_, config, _)| config.enabled)
                        .unwrap_or(true)
                })
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
            },
            Store::next_loop_tick_ms,
        )
    }

    pub fn due_task_retries(&self, now_ms: i64) -> Result<Vec<Task>> {
        self.rested_rows(
            "task_retries",
            now_ms,
            || {
                let mut statement = self.connection.prepare_cached(
                    "SELECT t.payload FROM task_block_retries r JOIN tasks t ON t.workspace_id=r.workspace_id AND t.task_id=r.task_id WHERE r.due_ms<=?1 AND t.blocking_kind='external_error' ORDER BY r.due_ms,t.ordinal",
                )?;
                let rows = statement.query_map([now_ms.max(0)], |row| decoded(row, 0))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            },
            Store::next_task_retry_ms,
        )
    }

    pub fn unresolved_task_reviews<T: DeserializeOwned>(
        &self,
    ) -> Result<Vec<UnresolvedTaskReview<T>>> {
        self.rested_rows(
            "task_reviews",
            0,
            || {
                let mut statement = self.connection.prepare_cached(
                    "SELECT workspace_id,task_id,attempt,version,stage,reviewer_pane_id,payload FROM task_reviews WHERE stage<?1 ORDER BY workspace_id,task_id,attempt",
                )?;
                let rows = statement.query_map([review_stage_rank(ReviewStage::Resolved)], |row| {
                    Ok(UnresolvedTaskReview {
                        workspace_id: row.get(0)?,
                        task_id: row.get(1)?,
                        attempt: row.get(2)?,
                        record: TaskReviewRecord {
                            version: row.get(3)?,
                            stage: decoded_review_stage(row, 4)?,
                            reviewer_pane_id: row.get(5)?,
                            payload: decoded(row, 6)?,
                        },
                    })
                })?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            },
            |_| Ok(None),
        )
    }

    pub fn tasks_with_pull_requests(&self) -> Result<Vec<Task>> {
        self.rested_rows(
            "task_pull_requests",
            0,
            || {
                let mut statement = self.connection.prepare_cached(
                    "SELECT payload FROM tasks WHERE state='in_review' AND json_type(payload,'$.pr')='object' ORDER BY workspace_id,ordinal",
                )?;
                let rows = statement.query_map([], |row| decoded(row, 0))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            },
            |_| Ok(None),
        )
    }
}
