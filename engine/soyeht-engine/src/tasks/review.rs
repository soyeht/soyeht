//! Durable execution of the independent contract-drift reviewer.

use super::{
    apply_complete, apply_review_return,
    drift::{DriftDecision, DriftFinding, evaluate_review, parse_findings, reviewer_prompt},
    events,
    review_request::ReviewRequest,
    revision::RevisionSnapshot,
};
use crate::{
    agents::{Running, state::State},
    core::Core,
    errors::{self, Result},
    model::{self, RunningPane, StoredPane},
    panes::persist_new_pane,
    ports::StorePort,
    screen::Screen,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use soyeht_protocol::{
    AttemptResult, Comment, Pane, PaneMode, PaneState, PromptOrigin, StateSource, Task,
    TaskCompleteParams, TaskMove, TaskState, TranscriptKind, TranscriptRole, event, limits, method,
};
use soyeht_store::{
    agents,
    database::Change,
    tasks::{RestartTaskReview, ReviewStage, TaskReviewRecord, UnresolvedTaskReview},
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionProposal {
    pub summary: String,
    pub contract_changes: Vec<soyeht_protocol::ContractChange>,
    pub out_of_scope_edits: Vec<soyeht_protocol::OutOfScopeEdit>,
}

impl CompletionProposal {
    fn parameters(&self, task: &Task) -> TaskCompleteParams {
        TaskCompleteParams {
            workspace_id: task.workspace_id.clone(),
            task_id: task.task_id.clone(),
            summary: self.summary.clone(),
            contract_changes: self.contract_changes.clone(),
            out_of_scope_edits: self.out_of_scope_edits.clone(),
            pane_id: task.owner_pane_id.clone(),
            session_generation: None,
            attempt: Some(task.current_attempt),
            idempotency_key: "review-resolution".into(),
        }
    }
}

impl From<&TaskCompleteParams> for CompletionProposal {
    fn from(value: &TaskCompleteParams) -> Self {
        Self {
            summary: value.summary.clone(),
            contract_changes: value.contract_changes.clone(),
            out_of_scope_edits: value.out_of_scope_edits.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewCheckpoint {
    pub executor_pane_id: String,
    pub prompt_id: String,
    pub revision: RevisionSnapshot,
    pub completion: Option<CompletionProposal>,
    pub findings: Option<Vec<DriftFinding>>,
    pub decision: Option<DriftDecision>,
}

struct PreparedReviewer {
    saved: StoredPane,
    state: State,
    screen: Screen,
    prompt: soyeht_protocol::PromptItem,
    checkpoint: ReviewCheckpoint,
}

impl Core {
    pub(crate) fn begin_task_review(
        &mut self,
        connection: &str,
        raw: &Value,
        request: ReviewRequest,
        revision: RevisionSnapshot,
    ) -> Result<Value> {
        let ReviewRequest {
            current,
            updated,
            actor,
            reason,
            repository: _,
        } = request;
        let existing = self.inactive_task_review(&current)?;
        let prepared = prepare_reviewer(self, &current, revision)?;
        let movement = TaskMove {
            actor,
            from: current.state.clone(),
            to: updated.state.clone(),
            reason,
            at: self.timestamp(),
        };
        let expected_task = current.version;
        let prior_version = existing.as_ref().map(|item| item.version);
        let result = json!({"task":updated});
        let start_events = review_start_events(&updated, &movement, &prepared.saved)?;
        let mutation = self.mutate(connection, method::TASK_REQUEST_REVIEW, raw, |change| {
            persist_review_start(
                change,
                &current,
                &updated,
                expected_task,
                &movement,
                &prepared,
                prior_version,
            )?;
            Ok((result, start_events))
        })?;
        if !mutation.repeated {
            self.install_reviewer(prepared)?;
        }
        Ok(mutation.result)
    }

    pub(crate) fn record_completion_proposal(
        &mut self,
        connection: &str,
        raw: &Value,
        current: &Task,
        params: &TaskCompleteParams,
    ) -> Result<Value> {
        let _ = apply_complete(current, params)?;
        let mut review: TaskReviewRecord<ReviewCheckpoint> = self
            .store
            .task_review(
                &current.workspace_id,
                &current.task_id,
                current.current_attempt,
            )
            .map_err(errors::storage)?
            .ok_or_else(|| errors::transition_refused("Task has no active reviewer"))?;
        if review.stage == ReviewStage::Resolved {
            return Err(errors::transition_refused(
                "Task review is already resolved",
            ));
        }
        checkpoint_completion(&mut review, params)?;
        let expected = review.version;
        let stage = review.stage;
        let checkpoint = review.payload;
        let workspace = current.workspace_id.clone();
        let task_id = current.task_id.clone();
        let attempt = current.current_attempt;
        let task = current.clone();
        let mutation = self.mutate(connection, method::TASK_COMPLETE, raw, move |change| {
            change.replace_task_review(
                &workspace,
                &task_id,
                attempt,
                expected,
                stage,
                &checkpoint,
            )?;
            Ok((json!({"task":task}), Vec::new()))
        })?;
        Ok(mutation.result)
    }

    pub(crate) fn advance_task_reviews(&mut self) -> Result<()> {
        let reviews: Vec<UnresolvedTaskReview<ReviewCheckpoint>> = self
            .store
            .unresolved_task_reviews()
            .map_err(errors::storage)?;
        for review in reviews {
            if review.record.stage == ReviewStage::PromptAccepted
                && self.reviewer_finished(&review.record)
            {
                self.classify_review(&review)?;
            }
            self.resolve_ready_review(&review.workspace_id, &review.task_id, review.attempt)?;
        }
        Ok(())
    }

    fn install_reviewer(&mut self, prepared: PreparedReviewer) -> Result<()> {
        let id = prepared.saved.pane.pane_id.clone();
        self.panes.insert(
            id.clone(),
            RunningPane {
                stored: prepared.saved,
                screen: prepared.screen,
            },
        );
        self.agents.insert(id.clone(), Running::new(prepared.state));
        if self.spawn(&id).is_err() {
            self.ptyd = None;
        }
        Ok(())
    }

    fn reviewer_finished(&self, review: &TaskReviewRecord<ReviewCheckpoint>) -> bool {
        let pane = self.panes.get(&review.reviewer_pane_id);
        let state = self.agents.get(&review.reviewer_pane_id);
        match (pane, state) {
            (Some(pane), Some(state)) if !pane.stored.closed => {
                (pane.stored.pane.state == PaneState::Idle && state.state.has_finished_turn)
                    || pane.stored.pane.state == PaneState::Dead
            }
            _ => true,
        }
    }

    fn classify_review(&mut self, review: &UnresolvedTaskReview<ReviewCheckpoint>) -> Result<()> {
        let mut checkpoint = review.record.payload.clone();
        checkpoint.findings =
            Some(self.review_findings(&review.record.reviewer_pane_id, &checkpoint.prompt_id)?);
        let workspace = review.workspace_id.clone();
        let task = review.task_id.clone();
        let attempt = review.attempt;
        let version = review.record.version;
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-review",
            &format!("classify:{workspace}:{task}:{attempt}:{version}"),
            "task-review-classification",
            |change| {
                change.replace_task_review(
                    &workspace,
                    &task,
                    attempt,
                    version,
                    ReviewStage::Classified,
                    &checkpoint,
                )?;
                Ok((json!({}), Vec::new()))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }

    fn review_findings(&self, pane: &str, prompt: &str) -> Result<Vec<DriftFinding>> {
        let mut before = None;
        let mut messages = Vec::new();
        loop {
            let page = self
                .store
                .agent_transcript(pane, before, limits::TRANSCRIPT_PAGE_MESSAGES)
                .map_err(errors::storage)?;
            messages.extend(page.items.into_iter().filter(|item| {
                item.prompt_id.as_deref() == Some(prompt)
                    && item.role == TranscriptRole::Assistant
                    && item.kind == TranscriptKind::Message
                    && item.complete
            }));
            let Some(cursor) = page.next_cursor else {
                break;
            };
            before = Some(
                cursor
                    .parse()
                    .map_err(|_| errors::internal("Stored transcript cursor is invalid"))?,
            );
        }
        let mut ordered = messages
            .into_iter()
            .map(|item| {
                item.seq
                    .parse::<u64>()
                    .map(|sequence| (sequence, item.text))
                    .map_err(|_| errors::internal("Stored transcript sequence is invalid"))
            })
            .collect::<Result<Vec<_>>>()?;
        ordered.sort_by_key(|(sequence, _)| *sequence);
        let text = ordered
            .into_iter()
            .map(|(_, text)| text)
            .collect::<String>();
        if text.len() > limits::JSON_BYTES {
            return Ok(vec![malformed_finding()]);
        }
        Ok(parse_findings(&text).unwrap_or_else(|| vec![malformed_finding()]))
    }

    fn resolve_ready_review(&mut self, workspace: &str, task_id: &str, attempt: u32) -> Result<()> {
        let review: TaskReviewRecord<ReviewCheckpoint> = self
            .store
            .task_review(workspace, task_id, attempt)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::internal("Task review disappeared"))?;
        if review.stage != ReviewStage::Classified
            || review.payload.completion.is_none()
            || review.payload.findings.is_none()
        {
            return Ok(());
        }
        self.persist_review_resolution(workspace, task_id, attempt, review)
    }

    fn persist_review_resolution(
        &mut self,
        workspace: &str,
        task_id: &str,
        attempt: u32,
        review: TaskReviewRecord<ReviewCheckpoint>,
    ) -> Result<()> {
        let task = self
            .store
            .task(workspace, task_id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::not_found("Reviewed task disappeared"))?;
        let version = review.version;
        let mut checkpoint = review.payload;
        let proposal = errors::invariant(checkpoint.completion.clone(), "Review proposal missing")?;
        let findings = errors::invariant(checkpoint.findings.as_ref(), "Review findings missing")?;
        let decision = evaluate_review(
            &checkpoint.revision,
            findings,
            &proposal.contract_changes,
            &proposal.out_of_scope_edits,
        );
        checkpoint.decision = Some(decision.clone());
        if task.state != TaskState::InReview
            || task.current_attempt != attempt
            || task.owner_pane_id.as_deref() != Some(&checkpoint.executor_pane_id)
        {
            return self.retire_obsolete_review(workspace, task_id, attempt, version, checkpoint);
        }
        self.apply_review_resolution(task, version, checkpoint, decision)
    }

    fn retire_obsolete_review(
        &mut self,
        workspace: &str,
        task_id: &str,
        attempt: u32,
        version: u32,
        checkpoint: ReviewCheckpoint,
    ) -> Result<()> {
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-review",
            &format!("obsolete:{workspace}:{task_id}:{attempt}:{version}"),
            "task-review-obsolete",
            |change| {
                change.replace_task_review(
                    workspace,
                    task_id,
                    attempt,
                    version,
                    ReviewStage::Resolved,
                    &checkpoint,
                )?;
                Ok((json!({}), Vec::new()))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }

    fn apply_review_resolution(
        &mut self,
        current: Task,
        review_version: u32,
        checkpoint: ReviewCheckpoint,
        decision: DriftDecision,
    ) -> Result<()> {
        let proposal =
            errors::invariant(checkpoint.completion.as_ref(), "Review proposal missing")?;
        let params = proposal.parameters(&current);
        let updated = if decision.approved {
            apply_complete(&current, &params)?
        } else {
            apply_review_return(&current, &params)?
        };
        let mut attempt = self.open_attempt(&current)?;
        if decision.approved {
            attempt.end = Some(self.timestamp());
            attempt.result = Some(AttemptResult::Completed);
            attempt.summary = Some(proposal.summary.clone());
        }
        let comment = review_comment(&current, &decision, &self.timestamp());
        let movement = review_movement(&current, &updated, &decision, &self.timestamp());
        let executor = (!decision.approved)
            .then(|| self.return_prompt(&current, &decision.comment))
            .transpose()?;
        self.commit_review_resolution(ReviewResolution {
            current,
            updated,
            attempt,
            review_version,
            checkpoint,
            comment,
            movement,
            executor,
        })
    }

    fn return_prompt(&self, task: &Task, comment: &str) -> Result<ReturnedExecutor> {
        let pane = task
            .owner_pane_id
            .as_deref()
            .ok_or_else(|| errors::internal("Returned review has no executor"))?;
        let saved = self
            .panes
            .get(pane)
            .filter(|value| !value.stored.closed)
            .ok_or_else(|| errors::internal("Returned review executor is unavailable"))?
            .stored
            .clone();
        let mut state = self.agent_state(pane)?;
        let prompt = state.enqueue(
            format!("Review returned this task. Address every line, then request review again.\n{comment}"),
            PromptOrigin::Automatic,
            self.timestamp(),
        )?;
        Ok(ReturnedExecutor {
            saved,
            state,
            prompt,
        })
    }

    fn commit_review_resolution(&mut self, resolution: ReviewResolution) -> Result<()> {
        let key = format!(
            "resolve:{}:{}:{}:{}",
            resolution.current.workspace_id,
            resolution.current.task_id,
            resolution.current.current_attempt,
            resolution.review_version
        );
        let events = resolution.events();
        let executor = resolution.executor.clone();
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-review",
            &key,
            "task-review-resolution",
            |change| resolution.persist(change, events),
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)?;
        if !mutation.repeated
            && let Some(executor) = executor
        {
            self.replace_agent_state(&executor.saved.pane.pane_id, executor.state)?;
        }
        Ok(())
    }
}

fn checkpoint_completion(
    review: &mut TaskReviewRecord<ReviewCheckpoint>,
    params: &TaskCompleteParams,
) -> Result<()> {
    let proposal = CompletionProposal::from(params);
    if review
        .payload
        .completion
        .as_ref()
        .is_some_and(|prior| prior != &proposal)
    {
        return Err(errors::stale_version("Completion proposal already differs"));
    }
    review.payload.completion = Some(proposal);
    Ok(())
}

#[derive(Clone)]
struct ReturnedExecutor {
    saved: StoredPane,
    state: State,
    prompt: soyeht_protocol::PromptItem,
}

struct ReviewResolution {
    current: Task,
    updated: Task,
    attempt: soyeht_protocol::Attempt,
    review_version: u32,
    checkpoint: ReviewCheckpoint,
    comment: Comment,
    movement: TaskMove,
    executor: Option<ReturnedExecutor>,
}

impl ReviewResolution {
    fn persist(
        self,
        change: &Change<'_>,
        events: Vec<(String, Value)>,
    ) -> soyeht_store::Result<(Value, Vec<(String, Value)>)> {
        change.replace_task(&self.updated, self.current.version)?;
        if self.attempt.end.is_some() {
            change.replace_attempt(
                &self.current.workspace_id,
                &self.current.task_id,
                &self.attempt,
            )?;
        }
        change.insert_move(
            &self.current.workspace_id,
            &self.current.task_id,
            &self.movement,
        )?;
        change.insert_comment(
            &self.current.workspace_id,
            &self.current.task_id,
            &self.comment,
        )?;
        change.replace_task_review(
            &self.current.workspace_id,
            &self.current.task_id,
            self.current.current_attempt,
            self.review_version,
            ReviewStage::Resolved,
            &self.checkpoint,
        )?;
        if let Some(executor) = self.executor {
            agents::save_runtime(
                change,
                &executor.saved.pane.pane_id,
                executor.saved.pane.generation,
                &executor.state,
            )?;
            agents::enqueue_prompt(change, &executor.saved.pane.pane_id, &executor.prompt)?;
        }
        Ok((json!({"task":self.updated}), events))
    }

    fn events(&self) -> Vec<(String, Value)> {
        vec![
            events::moved(&self.updated, &self.movement),
            (
                event::TASK_COMMENTED.into(),
                json!({"workspaceId":self.updated.workspace_id,"taskId":self.updated.task_id,"comment":self.comment}),
            ),
            events::updated(&self.updated),
            events::phrase_changed(&self.updated),
        ]
    }
}

pub(crate) fn review_repository(core: &Core, task: &Task) -> Result<PathBuf> {
    if let Some(worktree) = &task.worktree {
        return Ok(PathBuf::from(&worktree.path));
    }
    let pane = task
        .owner_pane_id
        .as_deref()
        .and_then(|id| core.panes.get(id))
        .ok_or_else(|| errors::stale_attempt("Task executor pane is unavailable"))?;
    Ok(PathBuf::from(&pane.stored.pane.cwd))
}

fn prepare_reviewer(
    core: &Core,
    task: &Task,
    revision: RevisionSnapshot,
) -> Result<PreparedReviewer> {
    let executor_id = task
        .owner_pane_id
        .as_deref()
        .ok_or_else(|| errors::stale_attempt("Task has no executor"))?;
    let executor = core.agent_state(executor_id)?;
    let mut state = State::new(
        executor.launch.clone(),
        executor.binary_digest.clone(),
        reviewer_mode(&executor.options),
    )?;
    copy_review_options(&executor, &mut state);
    state.disable_mcp();
    let repository = review_repository(core, task)?;
    let prompt = state.enqueue(
        reviewer_prompt(task, &revision),
        PromptOrigin::Automatic,
        core.timestamp(),
    )?;
    let saved = reviewer_pane(core, task, &state, &repository)?;
    let screen = Screen::open(
        &core.directory.join("sessions").join(&saved.pane.pane_id),
        true,
    )?;
    Ok(PreparedReviewer {
        checkpoint: ReviewCheckpoint {
            executor_pane_id: executor_id.into(),
            prompt_id: prompt.prompt_id.clone(),
            revision,
            completion: None,
            findings: None,
            decision: None,
        },
        saved,
        state,
        screen,
        prompt,
    })
}

/// A reviewer only reads: it starts in the harness's plan mode, else its first
/// standard mode, and never in a mode that approves everything.
fn reviewer_mode(options: &soyeht_acp::config::Options) -> Option<String> {
    let modes = &options.mode()?.options;
    ["plan", "standard"].iter().find_map(|kind| {
        modes
            .iter()
            .find(|choice| choice.kind.as_deref() == Some(*kind))
            .map(|choice| choice.value.clone())
    })
}

fn copy_review_options(executor: &State, reviewer: &mut State) {
    let shared = |id: &str| matches!(id, "model" | "effort");
    for option in executor.options.values.iter().filter(|o| shared(&o.id)) {
        reviewer
            .requested_options
            .insert(option.id.clone(), option.current_value.clone());
    }
}

fn reviewer_pane(core: &Core, task: &Task, state: &State, repository: &Path) -> Result<StoredPane> {
    let executor = task
        .owner_pane_id
        .as_deref()
        .and_then(|id| core.panes.get(id))
        .ok_or_else(|| errors::stale_attempt("Task executor pane is unavailable"))?;
    let cwd = repository
        .to_str()
        .ok_or_else(|| errors::invalid("Review directory must be UTF-8"))?;
    let pane = Pane {
        pane_id: Uuid::new_v4().to_string(),
        workspace_id: task.workspace_id.clone(),
        cwd: cwd.into(),
        name: format!("Review {}", task.task_id),
        mode: PaneMode::Agent,
        state: PaneState::Starting,
        source: StateSource::Protocol,
        confidence: 1.0,
        phrase: "Starting contract-drift review".into(),
        generation: 1,
        cols: executor.stored.pane.cols,
        rows: executor.stored.pane.rows,
        record: true,
        experiment_capture: Some(soyeht_protocol::PaneExperimentCapture {
            on: false,
            failure: None,
        }),
        pid: None,
        stream_id: None,
        agent: Some(state.public(core.now_ms())),
        url: None,
        title: None,
        viewport: None,
        incognito: None,
        writing_kind: None,
        writing_section: None,
        exit_code: None,
        web_session: None,
    };
    Ok(StoredPane {
        pane,
        closed: false,
        instance_key: Uuid::new_v4().to_string(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    })
}

fn persist_review_start(
    change: &Change<'_>,
    current: &Task,
    updated: &Task,
    expected_task: u32,
    movement: &TaskMove,
    prepared: &PreparedReviewer,
    prior_version: Option<u32>,
) -> soyeht_store::Result<()> {
    change.replace_task(updated, expected_task)?;
    change.insert_move(&current.workspace_id, &current.task_id, movement)?;
    persist_new_pane(change, &prepared.saved, Some(&prepared.state))?;
    agents::enqueue_prompt(change, &prepared.saved.pane.pane_id, &prepared.prompt)?;
    if let Some(version) = prior_version {
        change.restart_task_review(RestartTaskReview {
            workspace_id: &current.workspace_id,
            task_id: &current.task_id,
            attempt: current.current_attempt,
            expected_version: version,
            reviewer_pane_id: &prepared.saved.pane.pane_id,
            stage: ReviewStage::PromptAccepted,
            payload: &prepared.checkpoint,
        })?;
    } else {
        change.insert_task_review(
            &current.workspace_id,
            &current.task_id,
            current.current_attempt,
            &prepared.saved.pane.pane_id,
            &prepared.checkpoint,
        )?;
        change.replace_task_review(
            &current.workspace_id,
            &current.task_id,
            current.current_attempt,
            1,
            ReviewStage::PromptAccepted,
            &prepared.checkpoint,
        )?;
    }
    Ok(())
}

fn review_start_events(
    task: &Task,
    movement: &TaskMove,
    reviewer: &StoredPane,
) -> Result<Vec<(String, Value)>> {
    Ok(vec![
        events::moved(task, movement),
        events::updated(task),
        events::phrase_changed(task),
        model::pane_created(&reviewer.pane)?,
    ])
}

fn review_comment(task: &Task, decision: &DriftDecision, at: &str) -> Comment {
    Comment {
        comment_id: Uuid::new_v4().to_string(),
        task_id: task.task_id.clone(),
        author: "reviewer".into(),
        text: decision.comment.clone(),
        at: at.into(),
    }
}

fn review_movement(task: &Task, updated: &Task, decision: &DriftDecision, at: &str) -> TaskMove {
    TaskMove {
        actor: "reviewer".into(),
        from: task.state.clone(),
        to: updated.state.clone(),
        reason: if decision.approved {
            "contract-drift review approved"
        } else {
            "contract-drift review returned the task"
        }
        .into(),
        at: at.into(),
    }
}

fn malformed_finding() -> DriftFinding {
    DriftFinding {
        path: "reviewer".into(),
        name: "malformed_submission".into(),
        kind: super::revision::SurfaceKind::PublicFunction,
        label: soyeht_protocol::DriftLabel::Incomplete,
    }
}

#[cfg(test)]
mod reviewer_mode_tests {
    use super::reviewer_mode;
    use serde_json::json;
    use soyeht_acp::config::Options;

    fn modes(choices: &[(&str, &str)]) -> Options {
        let options: Vec<_> = choices
            .iter()
            .map(|(value, kind)| json!({"value":value,"name":value,"_meta":{"kind":kind}}))
            .collect();
        Options::from_response(&json!({"configOptions":[{"id":"mode","name":"Mode",
            "category":"mode","type":"select","currentValue":choices[0].0,"options":options}]}))
        .unwrap()
    }

    #[test]
    fn reviewers_prefer_plan_then_standard_and_never_full_access() {
        let claude = modes(&[
            ("bypassPermissions", "full_access"),
            ("default", "standard"),
            ("plan", "plan"),
        ]);
        assert_eq!(reviewer_mode(&claude).as_deref(), Some("plan"));
        let codex = modes(&[
            ("agent-full-access", "full_access"),
            ("agent", "auto_review"),
            ("read-only", "standard"),
        ]);
        assert_eq!(reviewer_mode(&codex).as_deref(), Some("read-only"));
        let open = modes(&[("everything", "full_access"), ("auto", "auto_review")]);
        assert_eq!(reviewer_mode(&open), None);
    }
}
