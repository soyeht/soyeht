//! Pure CI and review feedback decisions.

use crate::{
    agents::Changes,
    core::Core,
    errors::{self, Result},
    ports::StorePort,
    tasks::{
        PhraseFacts, phrase, review::review_repository, review_badge, revision::RevisionSnapshot,
    },
};
use serde_json::{Value, json};
use soyeht_protocol::{
    AttentionItem, Comment, PaneState, Task, TaskCheckState, TaskPullRequest, TaskReviewBadge,
    TaskReviewState,
};
use soyeht_store::{agents, database::Change, tasks::FeedbackState};
use uuid::Uuid;

const DELIVERY_LIMIT: u32 = 3;
const FEEDBACK_TEXT_LIMIT: usize = 64 * 1024;
const REVISION_POLL_INTERVAL_MS: i64 = 30_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedbackPaneFacts {
    pub owner_alive: bool,
    pub permission_pending: bool,
    pub human_draft_active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackMessage {
    pub prompt_id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeedbackDecision {
    pub task: Task,
    pub state: FeedbackState,
    pub message: Option<FeedbackMessage>,
    pub needs_attention: bool,
}

#[derive(Clone)]
pub(crate) struct TaskFeedbackChange {
    expected_version: u32,
    task: Task,
    state: FeedbackState,
}

impl TaskFeedbackChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::Result<()> {
        if self.task.version != self.expected_version {
            change.replace_task(&self.task, self.expected_version)?;
        }
        change.save_feedback(&self.task.workspace_id, &self.task.task_id, &self.state)
    }
}

#[derive(Clone)]
pub(crate) struct TaskEditorFeedbackChange {
    expected_version: u32,
    task: Task,
    comment: Comment,
}

impl TaskEditorFeedbackChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::Result<()> {
        change.replace_task(&self.task, self.expected_version)?;
        change.insert_comment(&self.task.workspace_id, &self.task.task_id, &self.comment)
    }

    pub(crate) fn task_id(&self) -> &str {
        &self.task.task_id
    }

    pub(crate) fn events(&self) -> Vec<(String, Value)> {
        vec![
            (
                "task.commented".into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,"comment":self.comment}),
            ),
            (
                "task.updated".into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,"version":self.task.version}),
            ),
            (
                "task.phrase_changed".into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,"phrase":self.task.phrase}),
            ),
        ]
    }
}

impl Core {
    pub(crate) fn prepare_editor_change_request(
        &self,
        pane_id: &str,
        comment_id: &str,
        text: &str,
    ) -> Result<Option<TaskEditorFeedbackChange>> {
        let Some(current) = self
            .store
            .active_task_for_pane(pane_id)
            .map_err(errors::storage)?
        else {
            return Ok(None);
        };
        let mut task = current.clone();
        task.version = task
            .version
            .checked_add(1)
            .ok_or_else(|| errors::internal("Task version exhausted"))?;
        task.phrase = "review requested changes".into();
        task.review_badge = Some(TaskReviewBadge::SystemTurn);
        let comment = Comment {
            comment_id: comment_id.into(),
            task_id: task.task_id.clone(),
            author: "you".into(),
            text: text.into(),
            at: self.timestamp(),
        };
        Ok(Some(TaskEditorFeedbackChange {
            expected_version: current.version,
            task,
            comment,
        }))
    }

    pub(crate) fn advance_task_feedback(&mut self) -> Result<()> {
        let now = self.now_ms();
        let tasks = self
            .store
            .tasks_with_pull_requests()
            .map_err(errors::storage)?;
        let mut first_error = None;
        for task in tasks {
            let key = (task.workspace_id.clone(), task.task_id.clone());
            if self
                .task_revision_due
                .get(&key)
                .is_some_and(|due| *due > now)
            {
                continue;
            }
            self.task_revision_due
                .insert(key, now.saturating_add(REVISION_POLL_INTERVAL_MS));
            if let Err(error) = self.advance_one_task_feedback(task)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn advance_one_task_feedback(&mut self, task: Task) -> Result<()> {
        let repository = review_repository(self, &task)?;
        let revision = self.revision.inspect(&repository)?;
        let prior = self
            .store
            .feedback_state(&task.workspace_id, &task.task_id)
            .map_err(errors::storage)?;
        let pane = task.owner_pane_id.clone();
        let facts = self.feedback_pane_facts(pane.as_deref())?;
        let decision = evaluate(&task, &revision, prior.as_ref(), &facts)?;
        let attention = self.feedback_attention(&decision, pane.as_deref())?;
        if let Some(pane) = pane
            .as_ref()
            .filter(|pane| self.agents.contains_key(pane.as_str()))
            .cloned()
        {
            return self.commit_feedback_to_pane(pane, task, decision, attention);
        }
        self.commit_feedback_without_pane(task, decision, attention)
    }

    fn feedback_pane_facts(&self, pane: Option<&str>) -> Result<FeedbackPaneFacts> {
        let Some(pane) = pane else {
            return Ok(FeedbackPaneFacts::default());
        };
        let owner_alive = self.panes.get(pane).is_some_and(|running| {
            !running.stored.closed && running.stored.pane.state != PaneState::Dead
        });
        let permission_pending = !self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
            .is_empty();
        let human_draft_active = self
            .agents
            .get(pane)
            .is_some_and(|running| running.state.draft_until_ms > self.now_ms());
        Ok(FeedbackPaneFacts {
            owner_alive,
            permission_pending,
            human_draft_active,
        })
    }

    fn feedback_attention(
        &self,
        decision: &FeedbackDecision,
        pane: Option<&str>,
    ) -> Result<AttentionChange> {
        let id = format!("task:{}:feedback", decision.task.task_id);
        let prior = agents::attention(&self.store, &decision.task.workspace_id)
            .map_err(errors::storage)?
            .into_iter()
            .find(|item| item.attention_id == id);
        let next = if decision.needs_attention {
            pane.map(|pane| AttentionItem {
                attention_id: id,
                pane_id: pane.into(),
                request_id: None,
                message: bounded_owned(
                    &format!("{}: {}", decision.task.task_id, decision.task.phrase),
                    1024,
                ),
                created_at: prior
                    .as_ref()
                    .map_or_else(|| self.timestamp(), |item| item.created_at.clone()),
            })
        } else {
            None
        };
        Ok(AttentionChange { prior, next })
    }

    fn commit_feedback_to_pane(
        &mut self,
        pane: String,
        current: Task,
        decision: FeedbackDecision,
        attention: AttentionChange,
    ) -> Result<()> {
        let mut state = self.agent_state(&pane)?;
        let prompt = decision
            .message
            .as_ref()
            .map(|message| {
                state.enqueue_feedback(
                    message.prompt_id.clone(),
                    message.text.clone(),
                    self.timestamp(),
                )
            })
            .transpose()?;
        let events = feedback_events(
            &current,
            &decision.task,
            prompt.as_ref().map(|prompt| (prompt, state.queue.len())),
            &attention,
        );
        let mut changes = Changes {
            events,
            task_feedback: Some(feedback_change(&current, &decision)),
            ..Changes::default()
        };
        apply_attention_change(&mut changes, attention);
        let saved = self.panes[&pane].stored.clone();
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }

    fn commit_feedback_without_pane(
        &mut self,
        current: Task,
        decision: FeedbackDecision,
        attention: AttentionChange,
    ) -> Result<()> {
        let key = Uuid::new_v4().to_string();
        let events = feedback_events(&current, &decision.task, None, &attention);
        let update = feedback_change(&current, &decision);
        let workspace = current.workspace_id;
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-feedback",
            &key,
            "task-feedback",
            |change| {
                update.persist(change)?;
                persist_attention_change(change, &workspace, &attention)?;
                let mut committed = events;
                if attention.next != attention.prior {
                    let items = agents::attention_in_change(change, &workspace)?;
                    committed.push((
                        "attention.changed".into(),
                        json!({"workspaceId":workspace,"count":items.len(),"items":items}),
                    ));
                }
                Ok((json!({}), committed))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }
}

#[derive(Clone)]
struct AttentionChange {
    prior: Option<AttentionItem>,
    next: Option<AttentionItem>,
}

fn feedback_change(current: &Task, decision: &FeedbackDecision) -> TaskFeedbackChange {
    TaskFeedbackChange {
        expected_version: current.version,
        task: decision.task.clone(),
        state: decision.state.clone(),
    }
}

fn apply_attention_change(changes: &mut Changes, attention: AttentionChange) {
    if attention.next != attention.prior {
        if let Some(item) = attention.next {
            changes.attention.push(item);
        } else if let Some(item) = attention.prior {
            changes.remove_attention.push(item.attention_id);
        }
    }
}

fn persist_attention_change(
    change: &Change<'_>,
    workspace: &str,
    attention: &AttentionChange,
) -> soyeht_store::Result<()> {
    if attention.next == attention.prior {
        return Ok(());
    }
    if let Some(item) = &attention.next {
        agents::save_attention(change, workspace, item)?;
    } else if let Some(item) = &attention.prior {
        change.remove_attention(&item.pane_id, &item.attention_id)?;
    }
    Ok(())
}

fn feedback_events(
    current: &Task,
    updated: &Task,
    prompt: Option<(&soyeht_protocol::PromptItem, usize)>,
    attention: &AttentionChange,
) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    if current != updated {
        events.push((
            "task.updated".into(),
            json!({"workspaceId":updated.workspace_id,"taskId":updated.task_id,"version":updated.version}),
        ));
        if current.phrase != updated.phrase {
            events.push((
                "task.phrase_changed".into(),
                json!({"workspaceId":updated.workspace_id,"taskId":updated.task_id,"phrase":updated.phrase}),
            ));
        }
    }
    if let Some((prompt, count)) = prompt {
        events.push((
            "pane.queued".into(),
            json!({"paneId":updated.owner_pane_id,"n":count,"promptIds":[prompt.prompt_id],"draft":false}),
        ));
    }
    if attention.next.is_some() && attention.next != attention.prior {
        events.push((
            "task.attention".into(),
            json!({"workspaceId":updated.workspace_id,"taskId":updated.task_id,"n":1}),
        ));
    }
    events
}

pub fn evaluate(
    task: &Task,
    revision: &RevisionSnapshot,
    previous: Option<&FeedbackState>,
    pane: &FeedbackPaneFacts,
) -> Result<FeedbackDecision> {
    validate_revision(revision)?;
    let task = task_with_revision(task, revision, pane.owner_alive)?;
    if !needs_feedback(revision) {
        return Ok(FeedbackDecision {
            task,
            state: rearmed(previous, revision),
            message: None,
            needs_attention: false,
        });
    }
    let mut state = previous.cloned().unwrap_or_else(|| FeedbackState {
        fingerprint: String::new(),
        delivery_count: 0,
        armed: true,
    });
    let unseen = state.fingerprint != revision.fingerprint;
    let eligible = state.delivery_count < DELIVERY_LIMIT && (state.armed || unseen);
    let deferred = pane.permission_pending || pane.human_draft_active;
    if !eligible || deferred || !pane.owner_alive {
        return Ok(FeedbackDecision {
            task,
            state,
            message: None,
            needs_attention: !pane.owner_alive,
        });
    }
    state.delivery_count += 1;
    state.fingerprint.clone_from(&revision.fingerprint);
    state.armed = false;
    Ok(FeedbackDecision {
        message: Some(message(&task, revision, state.delivery_count)),
        task,
        state,
        needs_attention: false,
    })
}

fn validate_revision(revision: &RevisionSnapshot) -> Result<()> {
    if revision.head.trim().is_empty() || revision.fingerprint.trim().is_empty() {
        return Err(errors::invalid(
            "Revision head and fingerprint are required",
        ));
    }
    if needs_feedback(revision) && revision.summary.trim().is_empty() {
        return Err(errors::invalid("Failed revision summary is required"));
    }
    Ok(())
}

fn task_with_revision(task: &Task, revision: &RevisionSnapshot, owner_alive: bool) -> Result<Task> {
    let mut updated = task.clone();
    let url = task
        .pr
        .as_ref()
        .map(|pr| pr.url.clone())
        .ok_or_else(|| errors::invalid("Task has no pull request"))?;
    let pr = TaskPullRequest {
        url,
        check_state: revision.check_state.clone(),
        review_state: revision.review_state.clone(),
        head: Some(revision.head.clone()),
    };
    updated.pr = Some(pr);
    updated.review_badge = review_badge(&updated, owner_alive);
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    if updated != *task {
        updated.version = task
            .version
            .checked_add(1)
            .ok_or_else(|| errors::internal("Task version exhausted"))?;
    }
    Ok(updated)
}

fn needs_feedback(revision: &RevisionSnapshot) -> bool {
    matches!(
        revision.check_state,
        TaskCheckState::Red | TaskCheckState::Conflict
    ) || revision.review_state == TaskReviewState::ChangesRequested
}

fn rearmed(previous: Option<&FeedbackState>, revision: &RevisionSnapshot) -> FeedbackState {
    let mut state = previous.cloned().unwrap_or_else(|| FeedbackState {
        fingerprint: revision.fingerprint.clone(),
        delivery_count: 0,
        armed: true,
    });
    state.fingerprint.clone_from(&revision.fingerprint);
    state.armed = revision.check_state == TaskCheckState::Green;
    state
}

fn message(task: &Task, revision: &RevisionSnapshot, delivery: u32) -> FeedbackMessage {
    let text = bounded_owned(
        &format!(
            "{}\n\nLog tail:\n{}",
            revision.summary,
            bounded(&revision.log_tail, FEEDBACK_TEXT_LIMIT)
        ),
        soyeht_protocol::limits::PROMPT_BYTES,
    );
    FeedbackMessage {
        prompt_id: format!("task-feedback:{}:{delivery}", task.task_id),
        text,
    }
}

fn bounded_owned(text: &str, limit: usize) -> String {
    bounded(text, limit).to_owned()
}

fn bounded(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::{
            Running,
            state::{Phase, State},
        },
        model::{RunningPane, StoredPane},
        panes::persist_new_pane,
        ports::testing::FixedClock,
        screen::Screen,
        tasks::{
            revision::{RevisionPort, testing::ScriptedRevisionPort},
            rules::tests::task_fixture,
        },
    };
    use soyeht_protocol::{
        HarnessLaunch, Pane, PaneMode, PermissionMode, PromptOrigin, StateSource, TaskReviewBadge,
        TaskState, TaskWorktree, Workspace,
    };
    use std::{fs, path::PathBuf, sync::Arc};

    fn snapshot(
        check: TaskCheckState,
        review: TaskReviewState,
        fingerprint: &str,
    ) -> RevisionSnapshot {
        RevisionSnapshot {
            head: "abc123".into(),
            check_state: check,
            review_state: review,
            summary: "Required check failed".into(),
            log_tail: "last failure line".into(),
            fingerprint: fingerprint.into(),
            changed_paths: Vec::new(),
            changed_surfaces: Vec::new(),
        }
    }

    fn task() -> Task {
        let mut task = task_fixture(TaskState::InReview);
        task.pr = Some(TaskPullRequest {
            url: "https://example.invalid/change/1".into(),
            check_state: TaskCheckState::Pending,
            review_state: TaskReviewState::None,
            head: None,
        });
        task
    }

    #[test]
    fn red_check_is_delivered_once_per_fingerprint_and_three_times_per_task() {
        let mut previous = None;
        for number in 1..=4 {
            let revision = snapshot(
                TaskCheckState::Red,
                TaskReviewState::None,
                &format!("red-{number}"),
            );
            let decision = evaluate(
                &task(),
                &revision,
                previous.as_ref(),
                &FeedbackPaneFacts {
                    owner_alive: true,
                    ..FeedbackPaneFacts::default()
                },
            )
            .unwrap();
            assert_eq!(decision.message.is_some(), number <= DELIVERY_LIMIT);
            assert_eq!(decision.task.state, TaskState::InReview);
            assert_eq!(
                decision.task.review_badge,
                Some(TaskReviewBadge::SystemTurn)
            );
            previous = Some(decision.state);
        }
        assert_eq!(previous.unwrap().delivery_count, DELIVERY_LIMIT);
    }

    #[test]
    fn green_rearms_the_same_failure_but_draft_and_permission_defer_it() {
        let red = snapshot(TaskCheckState::Red, TaskReviewState::None, "same");
        let first = evaluate(
            &task(),
            &red,
            None,
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert!(first.message.is_some());
        let duplicate = evaluate(
            &first.task,
            &red,
            Some(&first.state),
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert!(duplicate.message.is_none());
        let green = snapshot(
            TaskCheckState::Green,
            TaskReviewState::AwaitingPerson,
            "green",
        );
        let rearmed = evaluate(
            &duplicate.task,
            &green,
            Some(&duplicate.state),
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert!(rearmed.state.armed);
        let deferred = evaluate(
            &rearmed.task,
            &red,
            Some(&rearmed.state),
            &FeedbackPaneFacts {
                owner_alive: true,
                permission_pending: true,
                human_draft_active: true,
            },
        )
        .unwrap();
        assert!(deferred.message.is_none());
        assert_eq!(deferred.state.delivery_count, 1);
        let delivered = evaluate(
            &deferred.task,
            &red,
            Some(&deferred.state),
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert!(delivered.message.is_some());
        assert_eq!(delivered.state.delivery_count, 2);
    }

    #[test]
    fn failed_check_without_a_live_owner_enters_attention() {
        let decision = evaluate(
            &task(),
            &snapshot(TaskCheckState::Conflict, TaskReviewState::None, "conflict"),
            None,
            &FeedbackPaneFacts::default(),
        )
        .unwrap();
        assert!(decision.needs_attention);
        assert!(decision.message.is_none());
        assert_eq!(decision.task.review_badge, Some(TaskReviewBadge::YourTurn));
    }

    #[test]
    fn revision_changes_increment_the_task_version_once() {
        let original = task();
        let decision = evaluate(
            &original,
            &snapshot(
                TaskCheckState::Green,
                TaskReviewState::AwaitingPerson,
                "green",
            ),
            None,
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert_eq!(decision.task.version, original.version + 1);
        assert_eq!(decision.task.review_badge, Some(TaskReviewBadge::YourTurn));

        let unchanged = evaluate(
            &decision.task,
            &snapshot(
                TaskCheckState::Green,
                TaskReviewState::AwaitingPerson,
                "green",
            ),
            Some(&decision.state),
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert_eq!(unchanged.task.version, decision.task.version);
    }

    #[test]
    fn scripted_revision_states_feed_the_rule_without_io() {
        let mut port = ScriptedRevisionPort::default();
        port.script(
            "https://example.invalid/change/1",
            vec![snapshot(
                TaskCheckState::Red,
                TaskReviewState::ChangesRequested,
                "review-1",
            )],
        );
        let revision = port
            .inspect(std::path::Path::new("https://example.invalid/change/1"))
            .unwrap();
        let decision = evaluate(
            &task(),
            &revision,
            None,
            &FeedbackPaneFacts {
                owner_alive: true,
                ..FeedbackPaneFacts::default()
            },
        )
        .unwrap();
        assert!(decision.message.unwrap().text.contains("last failure line"));
    }

    struct FeedbackProfile(PathBuf);

    impl FeedbackProfile {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("sn-feedback-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            fs::create_dir(path.join("repository")).unwrap();
            Self(path)
        }
    }

    impl Drop for FeedbackProfile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn feedback_core(profile: &FeedbackProfile, revisions: Vec<RevisionSnapshot>) -> Core {
        let repository = profile.0.join("repository");
        let mut state = State::new(
            HarnessLaunch {
                id: "test".into(),
                name: "Test".into(),
                command: "/test".into(),
                args: Vec::new(),
                version: "1".into(),
                environment: None,
            },
            "digest".into(),
            PermissionMode::Ask,
        )
        .unwrap();
        state.phase = Phase::Ready;
        state.requested_options.clear();
        state
            .enqueue_identified(
                "loop:waiting".into(),
                "pull work".into(),
                PromptOrigin::Automatic,
                "before".into(),
            )
            .unwrap();
        let pane = Pane {
            pane_id: "pane-1".into(),
            workspace_id: "workspace-1".into(),
            cwd: repository.to_string_lossy().into(),
            name: "Executor".into(),
            mode: PaneMode::Agent,
            state: PaneState::Idle,
            source: StateSource::Protocol,
            confidence: 1.0,
            phrase: "idle".into(),
            generation: 1,
            cols: 80,
            rows: 24,
            record: true,
            experiment_capture: None,
            pid: None,
            stream_id: None,
            agent: Some(state.public(10_000)),
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            writing_kind: None,
            writing_section: None,
            web_session: None,
        };
        let saved = StoredPane {
            pane,
            closed: false,
            instance_key: "instance-1".into(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let mut item = task();
        item.worktree = Some(TaskWorktree {
            path: repository.to_string_lossy().into(),
            branch: "task/ws-1".into(),
        });
        let queued = state.queue[0].clone();
        let workspace = Workspace {
            workspace_id: "workspace-1".into(),
            name: "Feedback".into(),
            path: repository.to_string_lossy().into(),
            layout: json!({}),
        };
        let mut core = Core::open(&profile.0).unwrap();
        StorePort::mutate(
            &mut core.store,
            "test",
            "seed-feedback",
            "seed-feedback",
            |change| {
                change.insert_workspace(&workspace)?;
                persist_new_pane(change, &saved, Some(&state))?;
                agents::enqueue_prompt(change, "pane-1", &queued)?;
                change.insert_task(&item)?;
                Ok((json!({}), Vec::new()))
            },
        )
        .unwrap();
        let screen = Screen::open(&profile.0.join("sessions/pane-1"), true).unwrap();
        core.panes.insert(
            "pane-1".into(),
            RunningPane {
                stored: saved,
                screen,
            },
        );
        core.agents.insert("pane-1".into(), Running::new(state));
        core.clock = Arc::new(FixedClock(10_000));
        let mut revision = ScriptedRevisionPort::default();
        revision.script(repository.to_string_lossy().as_ref(), revisions);
        core.revision = Box::new(revision);
        core
    }

    #[test]
    fn engine_feedback_is_durable_prioritized_deduplicated_and_enters_attention() {
        let profile = FeedbackProfile::new();
        let red = snapshot(TaskCheckState::Red, TaskReviewState::None, "red-1");
        let green = snapshot(
            TaskCheckState::Green,
            TaskReviewState::AwaitingPerson,
            "green-1",
        );
        let conflict = snapshot(
            TaskCheckState::Conflict,
            TaskReviewState::None,
            "conflict-1",
        );
        let mut core = feedback_core(&profile, vec![red, green, conflict.clone(), conflict]);

        core.advance_task_feedback().unwrap();
        assert_eq!(
            core.agents["pane-1"]
                .state
                .queue
                .iter()
                .map(|item| item.prompt_id.as_str())
                .collect::<Vec<_>>(),
            vec!["task-feedback:WS-1:1", "loop:waiting"]
        );
        assert_eq!(
            core.store
                .feedback_state("workspace-1", "WS-1")
                .unwrap()
                .unwrap()
                .delivery_count,
            1
        );
        core.advance_task_feedback().unwrap();
        assert_eq!(core.agents["pane-1"].state.queue.len(), 2);

        core.task_revision_due.clear();
        core.advance_task_feedback().unwrap();
        core.task_revision_due.clear();
        core.panes.get_mut("pane-1").unwrap().stored.pane.state = PaneState::Dead;
        core.advance_task_feedback().unwrap();
        let before = agents::attention(&core.store, "workspace-1").unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(
            core.store.task("workspace-1", "WS-1").unwrap().unwrap().pr,
            Some(TaskPullRequest {
                url: "https://example.invalid/change/1".into(),
                check_state: TaskCheckState::Conflict,
                review_state: TaskReviewState::None,
                head: Some("abc123".into()),
            })
        );
        core.task_revision_due.clear();
        core.advance_task_feedback().unwrap();
        assert_eq!(
            agents::attention(&core.store, "workspace-1").unwrap(),
            before
        );
    }
}
