//! Pure CI and review feedback decisions.

use crate::{
    errors::{self, Result},
    tasks::{PhraseFacts, phrase, review_badge, revision::RevisionSnapshot},
};
use soyeht_protocol::{Task, TaskCheckState, TaskPullRequest, TaskReviewState};
use soyeht_store::tasks::FeedbackState;

const DELIVERY_LIMIT: u32 = 3;
const FEEDBACK_TEXT_LIMIT: usize = 64 * 1024;

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
    let text = format!(
        "{}\n\nLog tail:\n{}",
        revision.summary,
        bounded(&revision.log_tail, FEEDBACK_TEXT_LIMIT)
    );
    FeedbackMessage {
        prompt_id: format!("task-feedback:{}:{delivery}", task.task_id),
        text,
    }
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
    use crate::tasks::{
        revision::{RevisionPort, testing::ScriptedRevisionPort},
        rules::tests::task_fixture,
    };
    use soyeht_protocol::{TaskReviewBadge, TaskState};

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
        let revision = port.inspect("https://example.invalid/change/1").unwrap();
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
}
