//! Named persistence operations for workspace tasks.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use soyeht_protocol::{
    Attempt, Comment, LoopConfig, Subitem, Task, TaskExecutor, TaskMove, TaskPriority, TaskState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackState {
    pub fingerprint: String,
    pub delivery_count: u32,
    pub armed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchCheckpoint {
    pub stage: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub pane_id: Option<String>,
}

pub struct TaskReservation<'value> {
    pub pane_id: &'value str,
    pub harness: &'value str,
    pub model: &'value str,
    pub started_at: &'value str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewStage {
    Prepared,
    PaneOpened,
    PromptAccepted,
    Classified,
    Resolved,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskReviewRecord<T> {
    pub version: u32,
    pub stage: ReviewStage,
    pub reviewer_pane_id: String,
    pub payload: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnresolvedTaskReview<T> {
    pub workspace_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub record: TaskReviewRecord<T>,
}

pub struct RestartTaskReview<'a, T> {
    pub workspace_id: &'a str,
    pub task_id: &'a str,
    pub attempt: u32,
    pub expected_version: u32,
    pub reviewer_pane_id: &'a str,
    pub stage: ReviewStage,
    pub payload: &'a T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommentPage {
    pub items: Vec<Comment>,
    pub next_before: Option<u64>,
}

fn decoded<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let payload: String = row.get(index)?;
    serde_json::from_str(&payload).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn state_name(state: &TaskState) -> &'static str {
    match state {
        TaskState::Todo => "todo",
        TaskState::InProgress => "in_progress",
        TaskState::InReview => "in_review",
        TaskState::Completed => "completed",
        TaskState::Cancelled => "cancelled",
    }
}

fn priority_rank(priority: &TaskPriority) -> u32 {
    match priority {
        TaskPriority::Urgent => 0,
        TaskPriority::High => 1,
        TaskPriority::Medium => 2,
        TaskPriority::Low => 3,
    }
}

fn review_stage_rank(stage: ReviewStage) -> u32 {
    match stage {
        ReviewStage::Prepared => 0,
        ReviewStage::PaneOpened => 1,
        ReviewStage::PromptAccepted => 2,
        ReviewStage::Classified => 3,
        ReviewStage::Resolved => 4,
    }
}

fn decoded_review_stage(row: &Row<'_>, index: usize) -> rusqlite::Result<ReviewStage> {
    match row.get::<_, u32>(index)? {
        0 => Ok(ReviewStage::Prepared),
        1 => Ok(ReviewStage::PaneOpened),
        2 => Ok(ReviewStage::PromptAccepted),
        3 => Ok(ReviewStage::Classified),
        4 => Ok(ReviewStage::Resolved),
        value => Err(rusqlite::Error::IntegralValueOutOfRange(
            index,
            i64::from(value),
        )),
    }
}

struct TaskColumns<'task> {
    priority_rank: u32,
    owner: Option<&'task str>,
    planned_owner: Option<&'task str>,
    harness: Option<&'task str>,
}

fn task_columns(task: &Task) -> TaskColumns<'_> {
    let harness = task.executor.as_ref().and_then(|executor| {
        (!executor.workspace_default)
            .then_some(executor.harness.as_deref())
            .flatten()
    });
    TaskColumns {
        priority_rank: priority_rank(&task.priority),
        owner: task.owner_pane_id.as_deref(),
        planned_owner: task.planned_owner.as_deref(),
        harness,
    }
}

impl Store {
    pub fn task(&self, workspace_id: &str, task_id: &str) -> Result<Option<Task>> {
        Ok(self
            .connection
            .query_row(
                "SELECT payload FROM tasks WHERE workspace_id=?1 AND task_id=?2",
                params![workspace_id, task_id],
                |row| decoded(row, 0),
            )
            .optional()?)
    }

    pub fn list_tasks(
        &self,
        workspace_id: &str,
        state: Option<&TaskState>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Task>> {
        if limit == 0 || limit > 500 {
            return Err(Error::InvalidTaskLimit);
        }
        let state = state.map(state_name);
        let mut statement = self.connection.prepare(
            "SELECT payload FROM tasks WHERE workspace_id=?1 AND (?2 IS NULL OR state=?2) ORDER BY priority_rank,ordinal LIMIT ?3 OFFSET ?4",
        )?;
        let rows = statement.query_map(params![workspace_id, state, limit, offset], |row| {
            decoded(row, 0)
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn tasks_with_pull_requests(&self) -> Result<Vec<Task>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM tasks WHERE state='in_review' AND json_type(payload,'$.pr')='object' ORDER BY workspace_id,ordinal",
        )?;
        let rows = statement.query_map([], |row| decoded(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn task_subitems(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Subitem>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM task_subitems WHERE workspace_id=?1 AND task_id=?2 ORDER BY ordinal",
        )?;
        let rows = statement.query_map(params![workspace_id, task_id], |row| decoded(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn task_comments(
        &self,
        workspace_id: &str,
        task_id: &str,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<Comment>> {
        Ok(self
            .task_comments_page(workspace_id, task_id, before, limit)?
            .items)
    }

    pub fn task_comments_page(
        &self,
        workspace_id: &str,
        task_id: &str,
        before: Option<u64>,
        limit: u32,
    ) -> Result<CommentPage> {
        if limit == 0 || limit > 500 {
            return Err(Error::InvalidTaskLimit);
        }
        let before = before.unwrap_or(i64::MAX as u64).min(i64::MAX as u64) as i64;
        let mut statement = self.connection.prepare(
            "SELECT ordinal,payload FROM task_comments WHERE workspace_id=?1 AND task_id=?2 AND ordinal<?3 ORDER BY ordinal DESC LIMIT ?4",
        )?;
        let rows = statement
            .query_map(params![workspace_id, task_id, before, limit + 1], |row| {
                Ok((row.get::<_, i64>(0)? as u64, decoded(row, 1)?))
            })?;
        let mut rows = rows.collect::<rusqlite::Result<Vec<(u64, Comment)>>>()?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next_before = has_more
            .then(|| rows.last().map(|(ordinal, _)| *ordinal))
            .flatten();
        Ok(CommentPage {
            items: rows.into_iter().map(|(_, comment)| comment).collect(),
            next_before,
        })
    }

    pub fn task_attempts(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Attempt>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM task_attempts WHERE workspace_id=?1 AND task_id=?2 ORDER BY attempt",
        )?;
        let rows = statement.query_map(params![workspace_id, task_id], |row| decoded(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn task_moves(&self, workspace_id: &str, task_id: &str) -> Result<Vec<TaskMove>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM task_moves WHERE workspace_id=?1 AND task_id=?2 ORDER BY ordinal",
        )?;
        let rows = statement.query_map(params![workspace_id, task_id], |row| decoded(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn task_dependencies(&self, workspace_id: &str, task_id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT dependency_task_id FROM task_dependencies WHERE workspace_id=?1 AND task_id=?2 ORDER BY dependency_task_id",
        )?;
        let rows = statement.query_map(params![workspace_id, task_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn dependency_path_exists(
        &self,
        workspace_id: &str,
        from_task_id: &str,
        to_task_id: &str,
    ) -> Result<bool> {
        Ok(self.connection.query_row(
            "WITH RECURSIVE path(task_id) AS (SELECT dependency_task_id FROM task_dependencies WHERE workspace_id=?1 AND task_id=?2 UNION SELECT d.dependency_task_id FROM task_dependencies d JOIN path p ON d.task_id=p.task_id WHERE d.workspace_id=?1) SELECT EXISTS(SELECT 1 FROM path WHERE task_id=?3)",
            params![workspace_id, from_task_id, to_task_id],
            |row| row.get(0),
        )?)
    }

    pub fn active_task_for_pane(&self, pane_id: &str) -> Result<Option<Task>> {
        Ok(self
            .connection
            .query_row(
                "SELECT payload FROM tasks WHERE owner_pane_id=?1 AND state IN ('in_progress','in_review') ORDER BY ordinal LIMIT 1",
                [pane_id],
                |row| decoded(row, 0),
            )
            .optional()?)
    }

    pub fn loop_config(&self, pane_id: &str) -> Result<Option<(LoopConfig, u64)>> {
        let raw: Option<(String, i64)> = self
            .connection
            .query_row(
                "SELECT payload,next_tick_ms FROM task_loops WHERE pane_id=?1",
                [pane_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        raw.map(|(payload, tick)| Ok((serde_json::from_str(&payload)?, tick as u64)))
            .transpose()
    }

    pub fn due_loop_configs(&self, now_ms: u64) -> Result<Vec<(String, LoopConfig, u64)>> {
        let mut statement = self.connection.prepare(
            "SELECT pane_id,payload,next_tick_ms FROM task_loops WHERE next_tick_ms<=?1 ORDER BY next_tick_ms,pane_id",
        )?;
        let rows = statement.query_map([now_ms.min(i64::MAX as u64) as i64], |row| {
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
    }

    pub fn feedback_state(
        &self,
        workspace_id: &str,
        task_id: &str,
    ) -> Result<Option<FeedbackState>> {
        Ok(self
            .connection
            .query_row(
                "SELECT fingerprint,delivery_count,armed FROM task_feedback WHERE workspace_id=?1 AND task_id=?2",
                params![workspace_id, task_id],
                |row| {
                    Ok(FeedbackState {
                        fingerprint: row.get(0)?,
                        delivery_count: row.get(1)?,
                        armed: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn blocked_tasks(&self) -> Result<Vec<Task>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM tasks WHERE blocking_kind IS NOT NULL ORDER BY workspace_id,ordinal",
        )?;
        let rows = statement.query_map([], |row| decoded(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn due_task_retries(&self, now_ms: i64) -> Result<Vec<Task>> {
        let mut statement = self.connection.prepare(
            "SELECT t.payload FROM task_block_retries r JOIN tasks t ON t.workspace_id=r.workspace_id AND t.task_id=r.task_id WHERE r.due_ms<=?1 AND t.blocking_kind='external_error' ORDER BY r.due_ms,t.ordinal",
        )?;
        let rows = statement.query_map([now_ms.max(0)], |row| decoded(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn dispatch_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
    ) -> Result<Option<DispatchCheckpoint>> {
        Ok(self
            .connection
            .query_row(
                "SELECT stage,worktree_path,branch,pane_id FROM task_dispatch WHERE workspace_id=?1 AND task_id=?2",
                params![workspace_id, task_id],
                |row| {
                    Ok(DispatchCheckpoint {
                        stage: row.get(0)?,
                        worktree_path: row.get(1)?,
                        branch: row.get(2)?,
                        pane_id: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn task_review<T: DeserializeOwned>(
        &self,
        workspace_id: &str,
        task_id: &str,
        attempt: u32,
    ) -> Result<Option<TaskReviewRecord<T>>> {
        Ok(self
            .connection
            .query_row(
                "SELECT version,stage,reviewer_pane_id,payload FROM task_reviews WHERE workspace_id=?1 AND task_id=?2 AND attempt=?3",
                params![workspace_id, task_id, attempt],
                |row| {
                    Ok(TaskReviewRecord {
                        version: row.get(0)?,
                        stage: decoded_review_stage(row, 1)?,
                        reviewer_pane_id: row.get(2)?,
                        payload: decoded(row, 3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn unresolved_task_reviews<T: DeserializeOwned>(
        &self,
    ) -> Result<Vec<UnresolvedTaskReview<T>>> {
        let mut statement = self.connection.prepare(
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
    }

    pub fn dispatch_checkpoints(&self) -> Result<Vec<(String, String, DispatchCheckpoint)>> {
        let mut statement = self.connection.prepare(
            "SELECT workspace_id,task_id,stage,worktree_path,branch,pane_id FROM task_dispatch ORDER BY workspace_id,task_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                DispatchCheckpoint {
                    stage: row.get(2)?,
                    worktree_path: row.get(3)?,
                    branch: row.get(4)?,
                    pane_id: row.get(5)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn record_dispatch_checkpoint(
        &mut self,
        workspace_id: &str,
        task_id: &str,
        checkpoint: &DispatchCheckpoint,
    ) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let change = Change { transaction };
        change.save_dispatch_checkpoint(workspace_id, task_id, checkpoint)?;
        change.transaction.commit()?;
        Ok(())
    }

    pub fn remove_dispatch_checkpoint(&mut self, workspace_id: &str, task_id: &str) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let change = Change { transaction };
        change.clear_dispatch_checkpoint(workspace_id, task_id)?;
        change.transaction.commit()?;
        Ok(())
    }
}

impl Change<'_> {
    pub fn allocate_task_id(&self, workspace_id: &str) -> Result<String> {
        let number: i64 = self.transaction.query_row(
            "INSERT INTO workspace_task_counters(workspace_id,next_number) VALUES (?1,2) ON CONFLICT(workspace_id) DO UPDATE SET next_number=next_number+1 RETURNING next_number-1",
            [workspace_id],
            |row| row.get(0),
        )?;
        if number <= 0 {
            return Err(Error::InvalidTaskNumber);
        }
        Ok(format!("WS-{number}"))
    }

    pub fn insert_task(&self, task: &Task) -> Result<()> {
        let columns = task_columns(task);
        self.transaction.execute(
            "INSERT INTO tasks(workspace_id,task_id,state,priority_rank,owner_pane_id,planned_owner,executor_harness,blocking_kind,version,payload) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![task.workspace_id,task.task_id,state_name(&task.state),columns.priority_rank,columns.owner,columns.planned_owner,columns.harness,task.blocking.as_ref().map(|value| blocking_name(&value.kind)),task.version,serde_json::to_string(task)?],
        )?;
        self.replace_dependencies(task)?;
        Ok(())
    }

    pub fn replace_task(&self, task: &Task, expected_version: u32) -> Result<()> {
        if task.version
            != expected_version
                .checked_add(1)
                .ok_or(Error::InvalidTaskVersion)?
        {
            return Err(Error::InvalidTaskVersion);
        }
        let columns = task_columns(task);
        let changed = self.transaction.execute(
            "UPDATE tasks SET state=?1,priority_rank=?2,owner_pane_id=?3,planned_owner=?4,executor_harness=?5,blocking_kind=?6,version=?7,payload=?8 WHERE workspace_id=?9 AND task_id=?10 AND version=?11",
            params![state_name(&task.state),columns.priority_rank,columns.owner,columns.planned_owner,columns.harness,task.blocking.as_ref().map(|value| blocking_name(&value.kind)),task.version,serde_json::to_string(task)?,task.workspace_id,task.task_id,expected_version],
        )?;
        if changed == 1 {
            self.replace_dependencies(task)?;
        }
        if changed != 1 {
            return Err(Error::TaskVersionConflict);
        }
        Ok(())
    }

    pub fn allocate_subitem_id(&self, workspace_id: &str, task_id: &str) -> Result<String> {
        let number: i64 = self.transaction.query_row(
            "SELECT COALESCE(MAX(ordinal),0)+1 FROM task_subitems WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, task_id],
            |row| row.get(0),
        )?;
        if number <= 0 {
            return Err(Error::InvalidTaskNumber);
        }
        Ok(format!("{task_id}-T{number:02}"))
    }

    fn replace_dependencies(&self, task: &Task) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM task_dependencies WHERE workspace_id=?1 AND task_id=?2",
            params![task.workspace_id, task.task_id],
        )?;
        for dependency in &task.dependencies {
            self.transaction.execute(
                "INSERT INTO task_dependencies(workspace_id,task_id,dependency_task_id) VALUES (?1,?2,?3)",
                params![task.workspace_id, task.task_id, dependency],
            )?;
        }
        Ok(())
    }

    pub fn reserve_next_task(
        &self,
        workspace_id: &str,
        pane_id: &str,
        harness: &str,
        model: &str,
        started_at: &str,
    ) -> Result<Option<Task>> {
        let raw: Option<String> = self.transaction.query_row(
            "SELECT t.payload FROM tasks t WHERE t.workspace_id=?1 AND t.state='todo' AND t.owner_pane_id IS NULL AND t.blocking_kind IS NULL AND (t.executor_harness IS NULL OR t.executor_harness=?3) AND NOT EXISTS (SELECT 1 FROM task_dependencies d JOIN tasks needed ON needed.workspace_id=d.workspace_id AND needed.task_id=d.dependency_task_id WHERE d.workspace_id=t.workspace_id AND d.task_id=t.task_id AND needed.state!='completed') ORDER BY CASE WHEN t.planned_owner=?2 THEN 0 ELSE 1 END,t.priority_rank,t.ordinal LIMIT 1",
            params![workspace_id, pane_id, harness],
            |row| row.get(0),
        ).optional()?;
        let Some(raw) = raw else { return Ok(None) };
        let task: Task = serde_json::from_str(&raw)?;
        self.reserve_task_record(
            task,
            &TaskReservation {
                pane_id,
                harness,
                model,
                started_at,
            },
        )
        .map(Some)
    }

    pub fn reserve_task(
        &self,
        workspace_id: &str,
        task_id: &str,
        preset: &TaskExecutor,
        reservation: &TaskReservation<'_>,
    ) -> Result<Task> {
        let raw: Option<String> = self
            .transaction
            .query_row(
                "SELECT payload FROM tasks WHERE workspace_id=?1 AND task_id=?2 AND state IN ('todo','in_review') AND owner_pane_id IS NULL AND blocking_kind IS NULL",
                params![workspace_id, task_id],
                |row| row.get(0),
            )
            .optional()?;
        let raw = raw.ok_or(Error::TaskReservationConflict)?;
        let mut task: Task = serde_json::from_str(&raw)?;
        task.executor = Some(preset.clone());
        self.reserve_task_record(task, reservation)
    }

    fn reserve_task_record(
        &self,
        mut task: Task,
        reservation: &TaskReservation<'_>,
    ) -> Result<Task> {
        let expected = task.version;
        let prior_state = state_name(&task.state);
        task.current_attempt = task
            .current_attempt
            .checked_add(1)
            .ok_or(Error::InvalidTaskNumber)?;
        task.version = task
            .version
            .checked_add(1)
            .ok_or(Error::InvalidTaskVersion)?;
        task.state = TaskState::InProgress;
        task.owner_pane_id = Some(reservation.pane_id.into());
        task.phrase = format!("in progress · attempt {}", task.current_attempt);
        let changed = self.transaction.execute(
            "UPDATE tasks SET state='in_progress',owner_pane_id=?1,version=?2,payload=?3 WHERE workspace_id=?4 AND task_id=?5 AND state=?6 AND owner_pane_id IS NULL AND blocking_kind IS NULL AND version=?7",
            params![reservation.pane_id, task.version, serde_json::to_string(&task)?, task.workspace_id, task.task_id, prior_state, expected],
        )?;
        if changed != 1 {
            return Err(Error::TaskReservationConflict);
        }
        let attempt = Attempt {
            n: task.current_attempt,
            pane_id: reservation.pane_id.into(),
            harness: reservation.harness.into(),
            model: reservation.model.into(),
            start: reservation.started_at.into(),
            end: None,
            result: None,
            summary: None,
        };
        self.insert_attempt(&task.workspace_id, &task.task_id, &attempt)?;
        Ok(task)
    }

    pub fn insert_subitem(
        &self,
        workspace_id: &str,
        task_id: &str,
        subitem: &Subitem,
    ) -> Result<()> {
        let ordinal: i64 = self.transaction.query_row(
            "SELECT COALESCE(MAX(ordinal),0)+1 FROM task_subitems WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, task_id],
            |row| row.get(0),
        )?;
        self.transaction.execute(
            "INSERT INTO task_subitems(workspace_id,task_id,subitem_id,ordinal,payload) VALUES (?1,?2,?3,?4,?5)",
            params![workspace_id,task_id,subitem.subitem_id,ordinal,serde_json::to_string(subitem)?],
        )?;
        Ok(())
    }

    pub fn replace_subitem(
        &self,
        workspace_id: &str,
        task_id: &str,
        subitem: &Subitem,
    ) -> Result<()> {
        let changed = self.transaction.execute(
            "UPDATE task_subitems SET payload=?1 WHERE workspace_id=?2 AND task_id=?3 AND subitem_id=?4",
            params![serde_json::to_string(subitem)?,workspace_id,task_id,subitem.subitem_id],
        )?;
        if changed != 1 {
            return Err(Error::SubitemNotFound);
        }
        Ok(())
    }

    pub fn insert_comment(
        &self,
        workspace_id: &str,
        task_id: &str,
        comment: &Comment,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_comments(workspace_id,task_id,payload) VALUES (?1,?2,?3)",
            params![workspace_id, task_id, serde_json::to_string(comment)?],
        )?;
        Ok(())
    }

    pub fn insert_attempt(
        &self,
        workspace_id: &str,
        task_id: &str,
        attempt: &Attempt,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_attempts(workspace_id,task_id,attempt,payload) VALUES (?1,?2,?3,?4)",
            params![
                workspace_id,
                task_id,
                attempt.n,
                serde_json::to_string(attempt)?
            ],
        )?;
        Ok(())
    }

    pub fn replace_attempt(
        &self,
        workspace_id: &str,
        task_id: &str,
        attempt: &Attempt,
    ) -> Result<()> {
        let changed = self.transaction.execute(
            "UPDATE task_attempts SET payload=?1 WHERE workspace_id=?2 AND task_id=?3 AND attempt=?4",
            params![serde_json::to_string(attempt)?,workspace_id,task_id,attempt.n],
        )?;
        if changed != 1 {
            return Err(Error::AttemptNotFound);
        }
        Ok(())
    }

    pub fn insert_move(
        &self,
        workspace_id: &str,
        task_id: &str,
        movement: &TaskMove,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_moves(workspace_id,task_id,payload) VALUES (?1,?2,?3)",
            params![workspace_id, task_id, serde_json::to_string(movement)?],
        )?;
        Ok(())
    }

    pub fn save_loop_config(
        &self,
        pane_id: &str,
        config: &LoopConfig,
        next_tick_ms: u64,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_loops(pane_id,payload,next_tick_ms) VALUES (?1,?2,?3) ON CONFLICT(pane_id) DO UPDATE SET payload=excluded.payload,next_tick_ms=excluded.next_tick_ms",
            params![pane_id,serde_json::to_string(config)?,next_tick_ms.min(i64::MAX as u64) as i64],
        )?;
        Ok(())
    }

    pub fn save_feedback(
        &self,
        workspace_id: &str,
        task_id: &str,
        state: &FeedbackState,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_feedback(workspace_id,task_id,fingerprint,delivery_count,armed) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(workspace_id,task_id) DO UPDATE SET fingerprint=excluded.fingerprint,delivery_count=excluded.delivery_count,armed=excluded.armed",
            params![workspace_id,task_id,state.fingerprint,state.delivery_count,state.armed],
        )?;
        Ok(())
    }

    pub fn schedule_task_retry(
        &self,
        workspace_id: &str,
        task_id: &str,
        due_ms: i64,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_block_retries(workspace_id,task_id,due_ms) VALUES (?1,?2,?3) ON CONFLICT(workspace_id,task_id) DO UPDATE SET due_ms=excluded.due_ms",
            params![workspace_id, task_id, due_ms.max(0)],
        )?;
        Ok(())
    }

    pub fn clear_task_retry(&self, workspace_id: &str, task_id: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM task_block_retries WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, task_id],
        )?;
        Ok(())
    }

    pub fn save_dispatch_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        checkpoint: &DispatchCheckpoint,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_dispatch(workspace_id,task_id,stage,worktree_path,branch,pane_id) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(workspace_id,task_id) DO UPDATE SET stage=excluded.stage,worktree_path=excluded.worktree_path,branch=excluded.branch,pane_id=excluded.pane_id",
            params![workspace_id,task_id,checkpoint.stage,checkpoint.worktree_path,checkpoint.branch,checkpoint.pane_id],
        )?;
        Ok(())
    }

    pub fn insert_task_review<T: Serialize>(
        &self,
        workspace_id: &str,
        task_id: &str,
        attempt: u32,
        reviewer_pane_id: &str,
        payload: &T,
    ) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO task_reviews(workspace_id,task_id,attempt,version,stage,reviewer_pane_id,payload) VALUES (?1,?2,?3,1,?4,?5,?6)",
            params![workspace_id,task_id,attempt,review_stage_rank(ReviewStage::Prepared),reviewer_pane_id,serde_json::to_string(payload)?],
        )?;
        Ok(())
    }

    pub fn replace_task_review<T: Serialize>(
        &self,
        workspace_id: &str,
        task_id: &str,
        attempt: u32,
        expected_version: u32,
        stage: ReviewStage,
        payload: &T,
    ) -> Result<()> {
        let version = expected_version
            .checked_add(1)
            .ok_or(Error::InvalidReviewVersion)?;
        let rank = review_stage_rank(stage);
        let changed = self.transaction.execute(
            "UPDATE task_reviews SET version=?1,stage=?2,payload=?3 WHERE workspace_id=?4 AND task_id=?5 AND attempt=?6 AND version=?7 AND stage<=?2",
            params![version,rank,serde_json::to_string(payload)?,workspace_id,task_id,attempt,expected_version],
        )?;
        if changed != 1 {
            return Err(Error::ReviewVersionConflict);
        }
        Ok(())
    }

    pub fn restart_task_review<T: Serialize>(&self, value: RestartTaskReview<'_, T>) -> Result<()> {
        let version = value
            .expected_version
            .checked_add(1)
            .ok_or(Error::InvalidReviewVersion)?;
        let changed = self.transaction.execute(
            "UPDATE task_reviews SET version=?1,stage=?2,reviewer_pane_id=?3,payload=?4 WHERE workspace_id=?5 AND task_id=?6 AND attempt=?7 AND version=?8 AND stage=?9",
            params![version,review_stage_rank(value.stage),value.reviewer_pane_id,serde_json::to_string(value.payload)?,value.workspace_id,value.task_id,value.attempt,value.expected_version,review_stage_rank(ReviewStage::Resolved)],
        )?;
        if changed != 1 {
            return Err(Error::ReviewVersionConflict);
        }
        Ok(())
    }

    pub fn clear_dispatch_checkpoint(&self, workspace_id: &str, task_id: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM task_dispatch WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, task_id],
        )?;
        Ok(())
    }
}

fn blocking_name(kind: &soyeht_protocol::BlockingKind) -> &'static str {
    match kind {
        soyeht_protocol::BlockingKind::Dependency => "dependency",
        soyeht_protocol::BlockingKind::MissingInformation => "missing_information",
        soyeht_protocol::BlockingKind::ExternalError => "external_error",
    }
}
