//! Named persistence operations for workspace tasks.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, Row, params};
use serde::de::DeserializeOwned;
use soyeht_protocol::{
    Attempt, Comment, LoopConfig, Subitem, Task, TaskMove, TaskPriority, TaskState,
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
        if limit == 0 || limit > 500 {
            return Err(Error::InvalidTaskLimit);
        }
        let before = before.unwrap_or(i64::MAX as u64).min(i64::MAX as u64) as i64;
        let mut statement = self.connection.prepare(
            "SELECT payload FROM task_comments WHERE workspace_id=?1 AND task_id=?2 AND ordinal<?3 ORDER BY ordinal DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(params![workspace_id, task_id, before, limit], |row| {
            decoded(row, 0)
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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

    pub fn replace_task(&self, task: &Task, expected_version: u32) -> Result<bool> {
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
        Ok(changed == 1)
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
        let mut task: Task = serde_json::from_str(&raw)?;
        let expected = task.version;
        task.current_attempt = task
            .current_attempt
            .checked_add(1)
            .ok_or(Error::InvalidTaskNumber)?;
        task.version = task
            .version
            .checked_add(1)
            .ok_or(Error::InvalidTaskVersion)?;
        task.state = TaskState::InProgress;
        task.owner_pane_id = Some(pane_id.into());
        task.phrase = format!("in progress · attempt {}", task.current_attempt);
        let changed = self.transaction.execute(
            "UPDATE tasks SET state='in_progress',owner_pane_id=?1,version=?2,payload=?3 WHERE workspace_id=?4 AND task_id=?5 AND state='todo' AND owner_pane_id IS NULL AND blocking_kind IS NULL AND version=?6",
            params![pane_id, task.version, serde_json::to_string(&task)?, workspace_id, task.task_id, expected],
        )?;
        if changed != 1 {
            return Err(Error::TaskReservationConflict);
        }
        let attempt = Attempt {
            n: task.current_attempt,
            pane_id: pane_id.into(),
            harness: harness.into(),
            model: model.into(),
            start: started_at.into(),
            end: None,
            result: None,
            summary: None,
        };
        self.insert_attempt(workspace_id, &task.task_id, &attempt)?;
        Ok(Some(task))
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
    ) -> Result<bool> {
        Ok(self.transaction.execute(
            "UPDATE task_subitems SET payload=?1 WHERE workspace_id=?2 AND task_id=?3 AND subitem_id=?4",
            params![serde_json::to_string(subitem)?,workspace_id,task_id,subitem.subitem_id],
        )? == 1)
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
    ) -> Result<bool> {
        Ok(self.transaction.execute(
            "UPDATE task_attempts SET payload=?1 WHERE workspace_id=?2 AND task_id=?3 AND attempt=?4",
            params![serde_json::to_string(attempt)?,workspace_id,task_id,attempt.n],
        )? == 1)
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
