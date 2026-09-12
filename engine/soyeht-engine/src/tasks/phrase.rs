use soyeht_protocol::{
    Blocking, PaneState, Task, TaskCheckState, TaskReviewBadge, TaskReviewState, TaskState,
};

#[derive(Default)]
pub struct PhraseFacts {
    pub pane_state: Option<PaneState>,
    pub pending_permission: bool,
    pub no_signal_minutes: Option<u32>,
    pub last_commit_minutes: Option<u32>,
}

fn blocked(blocking: &Blocking) -> String {
    format!("blocked: {}", blocking.reason)
}

pub fn phrase(task: &Task, facts: &PhraseFacts) -> String {
    if facts.pending_permission {
        return "waiting for your permission".into();
    }
    if let Some(minutes) = facts.no_signal_minutes {
        return format!("no signal for {minutes} min");
    }
    if task.pr.as_ref().is_some_and(|pr| {
        matches!(
            pr.check_state,
            TaskCheckState::Red | TaskCheckState::Conflict
        )
    }) {
        return "CI failed, fixing".into();
    }
    if task
        .pr
        .as_ref()
        .is_some_and(|pr| pr.review_state == TaskReviewState::ChangesRequested)
    {
        return "review requested changes".into();
    }
    if let Some(blocking) = &task.blocking {
        return blocked(blocking);
    }
    if task.state == TaskState::InProgress {
        return facts.last_commit_minutes.map_or_else(
            || format!("in progress · attempt {}", task.current_attempt),
            |minutes| format!("working · last commit {minutes} min ago"),
        );
    }
    match task.state {
        TaskState::Todo => "available to claim".into(),
        TaskState::InReview => "awaiting review".into(),
        TaskState::Completed => "completed".into(),
        TaskState::Cancelled => "cancelled".into(),
        TaskState::InProgress => format!("in progress · attempt {}", task.current_attempt),
    }
}

pub fn review_badge(task: &Task, owner_alive: bool) -> Option<TaskReviewBadge> {
    if task.state != TaskState::InReview {
        return None;
    }
    let system_turn = owner_alive
        && task.pr.as_ref().is_some_and(|pr| {
            matches!(
                pr.check_state,
                TaskCheckState::Red | TaskCheckState::Conflict
            ) || pr.review_state == TaskReviewState::ChangesRequested
        });
    Some(if system_turn {
        TaskReviewBadge::SystemTurn
    } else {
        TaskReviewBadge::YourTurn
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::rules::tests::task_fixture;
    use soyeht_protocol::{BlockingKind, TaskPullRequest};

    #[test]
    fn phrase_uses_the_contract_priority_and_places_blocking_before_availability() {
        let mut task = task_fixture(TaskState::Todo);
        task.blocking = Some(Blocking {
            kind: BlockingKind::MissingInformation,
            reason: "need a decision".into(),
            dependency_task_id: None,
            retry_count: None,
        });
        assert_eq!(
            phrase(&task, &PhraseFacts::default()),
            "blocked: need a decision"
        );

        let facts = PhraseFacts {
            pending_permission: true,
            no_signal_minutes: Some(12),
            ..PhraseFacts::default()
        };
        assert_eq!(phrase(&task, &facts), "waiting for your permission");
    }

    #[test]
    fn review_badge_assigns_the_turn_from_review_facts() {
        let mut task = task_fixture(TaskState::InReview);
        task.pr = Some(TaskPullRequest {
            url: "https://example.invalid/change/1".into(),
            check_state: TaskCheckState::Red,
            review_state: TaskReviewState::None,
            head: None,
        });
        assert_eq!(review_badge(&task, true), Some(TaskReviewBadge::SystemTurn));
        assert_eq!(review_badge(&task, false), Some(TaskReviewBadge::YourTurn));
    }
}
