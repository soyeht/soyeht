//! `task.request_review`: validated under the Core lock, git off it, then
//! validated again and started under it (ADR 0079).

use super::{
    api::parameters,
    apply_request_review,
    review::{ReviewCheckpoint, review_repository},
};
use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    offload::Offload,
};
use serde_json::Value;
use soyeht_protocol::{Task, TaskRequestReviewParams};
use soyeht_store::tasks::{ReviewStage, TaskReviewRecord};
use std::{path::PathBuf, time::Duration};

/// git runs several commands for a review snapshot, each with its own deadline.
const REVIEW_INSPECTION_TIMEOUT: Duration = Duration::from_secs(300);

/// A validated `task.request_review`, captured under the Core lock before
/// git inspects its repository off the lock (ADR 0079).
pub(crate) struct ReviewRequest {
    pub(crate) current: Task,
    pub(crate) updated: Task,
    pub(crate) actor: String,
    pub(crate) reason: String,
    pub(crate) repository: PathBuf,
}

impl Core {
    /// Validates under the Core lock, inspects the repository with git off
    /// it, then validates again and starts the reviewer under it (ADR 0079).
    pub(crate) fn task_request_review(&mut self, connection: &str, raw: &Value) -> Result<Outcome> {
        if let Some(result) = self.repeated_mutation(connection, "task.request_review", raw)? {
            return Ok(Outcome::Reply(result));
        }
        let params: TaskRequestReviewParams = parameters(raw)?;
        let repository = self.review_request(connection, raw, &params)?.repository;
        let revision = self.revision.clone();
        let connection = connection.to_owned();
        let raw = raw.clone();
        Ok(Outcome::Offload(Offload::new(
            "Task review inspection timed out",
            REVIEW_INSPECTION_TIMEOUT,
            {
                let repository = repository.clone();
                move || revision.with(|port| port.inspect(&repository))
            },
            move |core, snapshot| {
                let snapshot = snapshot?;
                // The task may have changed while git ran.
                let request = core.review_request(&connection, &raw, &params)?;
                if request.repository != repository {
                    return Err(errors::stale_attempt(
                        "Task repository changed during review inspection",
                    ));
                }
                core.begin_task_review(&connection, &raw, request, snapshot)
                    .map(Outcome::Reply)
            },
        )))
    }

    fn review_request(
        &self,
        connection: &str,
        raw: &Value,
        params: &TaskRequestReviewParams,
    ) -> Result<ReviewRequest> {
        let (actor, current) =
            self.executor_task(connection, raw, &params.workspace_id, &params.task_id)?;
        let subitems = self
            .store
            .task_subitems(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?;
        let updated = apply_request_review(&current, &subitems)?;
        self.inactive_task_review(&current)?;
        Ok(ReviewRequest {
            repository: review_repository(self, &current)?,
            actor: self.actor_name(&actor)?,
            reason: params
                .reason
                .clone()
                .unwrap_or_else(|| "review requested".into()),
            current,
            updated,
        })
    }

    /// The task's review record, refusing a review that is still active.
    pub(crate) fn inactive_task_review(
        &self,
        task: &Task,
    ) -> Result<Option<TaskReviewRecord<ReviewCheckpoint>>> {
        let existing: Option<TaskReviewRecord<ReviewCheckpoint>> = self
            .store
            .task_review(&task.workspace_id, &task.task_id, task.current_attempt)
            .map_err(errors::storage)?;
        if existing
            .as_ref()
            .is_some_and(|item| item.stage != ReviewStage::Resolved)
        {
            return Err(errors::transition_refused("Task review is already active"));
        }
        Ok(existing)
    }
}
