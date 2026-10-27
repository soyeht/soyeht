use super::*;
use crate::{
    agents::Running,
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
    Pane, PaneMode, PromptOrigin, TaskReviewBadge, TaskState, TaskWorktree, Workspace,
};
use std::{fs, path::PathBuf, sync::Arc};

fn snapshot(check: TaskCheckState, review: TaskReviewState, fingerprint: &str) -> RevisionSnapshot {
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
    let mut state = crate::test_panes::ready_agent_state();
    state
        .enqueue_identified(
            "loop:waiting".into(),
            "pull work".into(),
            PromptOrigin::Automatic,
            "before".into(),
        )
        .unwrap();
    let pane = Pane {
        name: "Executor".into(),
        phrase: "idle".into(),
        record: true,
        agent: Some(state.public(10_000)),
        ..crate::test_panes::pane(
            "pane-1",
            "workspace-1",
            PaneMode::Agent,
            &repository.to_string_lossy(),
        )
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
    core.revision = crate::offload::SharedPort::new(Box::new(revision));
    core.background = crate::offload::Background::inline();
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
