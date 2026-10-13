//! `task.worktree.clean`: validated under the Core lock, `wt` and `lsof` off it
//! (ADR 0079).

use super::worktree::CleanOutcome;
use crate::{
    dispatch::Outcome,
    errors::{self, Result},
    offload::Offload,
};
use serde_json::Value;
use soyeht_protocol::{Role, TaskWorktreeCleanParams, TaskWorktreeCleanResult, method};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// A list, a process scan and a removal, plus a wait behind other `wt` users.
const CLEAN_TIMEOUT: Duration = Duration::from_secs(300);

impl crate::core::Core {
    /// Validates under the Core lock, runs `wt` and `lsof` off it, and
    /// records the result under it (ADR 0079).
    pub(crate) fn task_worktree_clean(&mut self, connection: &str, raw: &Value) -> Result<Outcome> {
        if let Some(result) =
            self.repeated_mutation(connection, method::TASK_WORKTREE_CLEAN, raw)?
        {
            return Ok(Outcome::Reply(result));
        }
        let params: TaskWorktreeCleanParams = crate::tasks::api::parameters(raw)?;
        let actor = self.task_actor(connection)?;
        if !matches!(actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Err(errors::forbidden_role("Worktree cleanup requires a person"));
        }
        let task = crate::tasks::api::task(&self.store, &params.workspace_id, &params.task_id)?;
        let Some(worktree) = task.worktree else {
            return self
                .persist_clean_result(
                    connection,
                    raw,
                    CleanOutcome {
                        cleaned: false,
                        reason: Some("Task has no worktree".into()),
                    },
                )
                .map(Outcome::Reply);
        };
        let repository =
            PathBuf::from(crate::model::workspace(&self.store, &params.workspace_id)?.path);
        let worktrees = self.worktrees.clone();
        let connection = connection.to_owned();
        let raw = raw.clone();
        Ok(Outcome::Offload(Offload::new(
            "Worktree cleanup timed out",
            CLEAN_TIMEOUT,
            move || {
                worktrees.with(|port| {
                    port.clean(&repository, Path::new(&worktree.path), &worktree.branch)
                })
            },
            move |core, outcome| {
                core.persist_clean_result(&connection, &raw, outcome?)
                    .map(Outcome::Reply)
            },
        )))
    }

    fn persist_clean_result(
        &mut self,
        connection: &str,
        raw: &Value,
        outcome: CleanOutcome,
    ) -> Result<Value> {
        let result = serde_json::to_value(TaskWorktreeCleanResult {
            cleaned: outcome.cleaned,
            reason: outcome.reason,
        })
        .map_err(|_| errors::internal("Worktree cleanup result serialization failed"))?;
        let stored = result.clone();
        let mutation = self.mutate(connection, method::TASK_WORKTREE_CLEAN, raw, move |_| {
            Ok((stored, Vec::new()))
        })?;
        Ok(mutation.result)
    }
}
