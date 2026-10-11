#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde::{Deserialize, Serialize};
use serde_json::json;
use soyeht_protocol::{
    Attempt, AttemptResult, Comment, LoopConfig, LoopStopWhen, Pane, PaneMode, PaneState,
    StateSource, Subitem, SubitemState, Task, TaskExecutor, TaskMove, TaskPriority, TaskState,
    Workspace,
};
use soyeht_store::{
    database::Store,
    domain::PaneRecord,
    tasks::{
        DispatchCheckpoint, FeedbackState, RestartTaskReview, ReviewStage, TaskReservation,
        TaskReviewRecord, UnresolvedTaskReview,
    },
};
use std::{fs, path::PathBuf, sync::Arc, thread};
use uuid::Uuid;

struct Profile(PathBuf);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ReviewPayload {
    proposal: Option<String>,
    findings: Option<Vec<String>>,
}

impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sn-task-store-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn store(&self) -> Store {
        Store::open(&self.0).unwrap()
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn workspace() -> Workspace {
    Workspace {
        workspace_id: "workspace-1".into(),
        name: "Tasks".into(),
        path: "/tmp/tasks".into(),
        layout: json!({}),
    }
}

fn pane(id: &str) -> PaneRecord {
    PaneRecord {
        pane: Pane {
            pane_id: id.into(),
            workspace_id: "workspace-1".into(),
            name: id.into(),
            cwd: "/tmp/tasks".into(),
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
            agent: None,
            url: None,
            title: None,
            viewport: None,
            incognito: None,
            writing_kind: None,
            writing_section: None,
            web_session: None,
        },
        closed: false,
        instance_key: format!("instance-{id}"),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    }
}

fn task(id: &str, priority: TaskPriority) -> Task {
    Task {
        workspace_id: "workspace-1".into(),
        task_id: id.into(),
        title: format!("Task {id}"),
        description: "Implement the change".into(),
        how_to_verify: Some("Run the test".into()),
        must_not_change: "Keep existing behavior".into(),
        state: TaskState::Todo,
        priority,
        planned_owner: None,
        owner_pane_id: None,
        executor: None,
        dependencies: vec![],
        worktree: None,
        current_attempt: 0,
        blocking: None,
        pr: None,
        pr_contract: false,
        created_at: "2026-09-11T00:00:00Z".into(),
        created_by: "you".into(),
        version: 1,
        phrase: "available to claim".into(),
        review_badge: None,
        summary: None,
        contract_changes: None,
        out_of_scope_edits: None,
    }
}

fn seed(store: &mut Store) {
    store
        .mutate("test", "seed", "seed", |change| {
            change.insert_workspace(&workspace())?;
            change.insert_pane(&pane("pane-a"))?;
            change.insert_pane(&pane("pane-b"))?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn insert(store: &mut Store, item: &Task, key: &str) {
    store
        .mutate("test", key, key, |change| {
            change.insert_task(item)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

#[test]
fn pull_request_feedback_query_returns_only_review_cards_with_a_pr() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let mut review = task("WS-1", TaskPriority::Medium);
    review.state = TaskState::InReview;
    review.pr = Some(soyeht_protocol::TaskPullRequest {
        url: "https://example.invalid/pull/1".into(),
        check_state: soyeht_protocol::TaskCheckState::Pending,
        review_state: soyeht_protocol::TaskReviewState::None,
        head: None,
    });
    let mut todo = review.clone();
    todo.task_id = "WS-2".into();
    todo.state = TaskState::Todo;
    let mut no_pr = review.clone();
    no_pr.task_id = "WS-3".into();
    no_pr.pr = None;
    insert(&mut store, &review, "insert-review-pr");
    insert(&mut store, &todo, "insert-todo-pr");
    insert(&mut store, &no_pr, "insert-review-no-pr");

    assert_eq!(store.tasks_with_pull_requests().unwrap(), vec![review]);
}

fn insert_review_task(store: &mut Store, id: &str, pane_id: &str) {
    let mut item = task(id, TaskPriority::Medium);
    item.current_attempt = 1;
    insert(store, &item, &format!("task-{id}"));
    let attempt = Attempt {
        n: 1,
        pane_id: pane_id.into(),
        harness: "harness".into(),
        model: "model".into(),
        start: "start".into(),
        end: None,
        result: None,
        summary: None,
    };
    store
        .mutate("test", &format!("attempt-{id}"), id, |change| {
            change.insert_attempt("workspace-1", id, &attempt)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn prepare_review(store: &mut Store, task_id: &str, reviewer: &str) {
    insert_review_task(store, task_id, "pane-a");
    store
        .mutate("test", &format!("review-{task_id}"), task_id, |change| {
            change.insert_task_review(
                "workspace-1",
                task_id,
                1,
                reviewer,
                &ReviewPayload {
                    proposal: None,
                    findings: None,
                },
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    replace_review(
        store,
        &format!("prompt-{task_id}"),
        task_id,
        1,
        ReviewStage::PromptAccepted,
        None,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn replace_review(
    store: &mut Store,
    key: &str,
    task_id: &str,
    expected_version: u32,
    stage: ReviewStage,
    proposal: Option<&str>,
    findings: Option<&[&str]>,
) {
    let payload = ReviewPayload {
        proposal: proposal.map(String::from),
        findings: findings.map(|items| items.iter().map(|item| String::from(*item)).collect()),
    };
    store
        .mutate("test", key, key, |change| {
            change.replace_task_review(
                "workspace-1",
                task_id,
                1,
                expected_version,
                stage,
                &payload,
            )?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn assert_complete_review(store: &Store, task_id: &str, reviewer: &str) {
    let record: TaskReviewRecord<ReviewPayload> = store
        .task_review("workspace-1", task_id, 1)
        .unwrap()
        .unwrap();
    assert_eq!(record.version, 4);
    assert_eq!(record.stage, ReviewStage::Classified);
    assert_eq!(record.reviewer_pane_id, reviewer);
    assert_eq!(record.payload.proposal.as_deref(), Some("summary"));
    assert_eq!(record.payload.findings, Some(vec!["equivalent".into()]));
}

#[test]
fn task_ids_and_filtered_pages_are_scoped_to_a_real_workspace() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let ids = store
        .mutate("test", "ids", "ids", |change| {
            let first = change.allocate_task_id("workspace-1")?;
            let second = change.allocate_task_id("workspace-1")?;
            Ok((json!([first, second]), vec![]))
        })
        .unwrap()
        .result;
    assert_eq!(ids, json!(["WS-1", "WS-2"]));
    insert(&mut store, &task("WS-1", TaskPriority::Low), "task-low");
    insert(
        &mut store,
        &task("WS-2", TaskPriority::Urgent),
        "task-urgent",
    );
    assert_eq!(
        store.task("workspace-1", "WS-2").unwrap().unwrap().title,
        "Task WS-2"
    );
    let page = store
        .list_tasks("workspace-1", Some(&TaskState::Todo), 1, 0)
        .unwrap();
    assert_eq!(page[0].task_id, "WS-2");
    assert!(store.list_tasks("workspace-1", None, 0, 0).is_err());
}

#[test]
fn dependency_queries_and_versioned_replacement_use_the_real_graph() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "first");
    let mut second = task("WS-2", TaskPriority::Medium);
    second.dependencies.push("WS-1".into());
    insert(&mut store, &second, "second");
    assert_eq!(
        store.task_dependencies("workspace-1", "WS-2").unwrap(),
        ["WS-1"]
    );
    assert!(
        store
            .dependency_path_exists("workspace-1", "WS-2", "WS-1")
            .unwrap()
    );
    assert!(
        !store
            .dependency_path_exists("workspace-1", "WS-1", "WS-2")
            .unwrap()
    );
    second.title = "Updated".into();
    second.version = 2;
    store
        .mutate("test", "update", "update", |change| {
            change.replace_task(&second, 1)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store.task("workspace-1", "WS-2").unwrap().unwrap().version,
        2
    );
}

#[test]
fn task_details_round_trip_as_separate_named_relations() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "task");
    let mut subitem = Subitem {
        subitem_id: "WS-1-T01".into(),
        task_id: "WS-1".into(),
        text: "First step".into(),
        agent: Some("pane-a".into()),
        instructions: Some("Do it".into()),
        state: SubitemState::Open,
    };
    let comment = Comment {
        comment_id: "comment-1".into(),
        task_id: "WS-1".into(),
        author: "agent".into(),
        text: "Started".into(),
        at: "start".into(),
    };
    let attempt = Attempt {
        n: 1,
        pane_id: "pane-a".into(),
        harness: "harness".into(),
        model: "model".into(),
        start: "start".into(),
        end: None,
        result: None,
        summary: None,
    };
    let movement = TaskMove {
        actor: "you".into(),
        from: TaskState::Todo,
        to: TaskState::Cancelled,
        reason: "No longer needed".into(),
        at: "later".into(),
    };
    store
        .mutate("test", "details", "details", |change| {
            change.insert_subitem("workspace-1", "WS-1", &subitem)?;
            change.insert_comment("workspace-1", "WS-1", &comment)?;
            change.insert_attempt("workspace-1", "WS-1", &attempt)?;
            change.insert_move("workspace-1", "WS-1", &movement)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    subitem.state = SubitemState::Done;
    let mut closed = attempt;
    closed.end = Some("end".into());
    closed.result = Some(AttemptResult::Completed);
    store
        .mutate("test", "detail-updates", "detail-updates", |change| {
            change.replace_subitem("workspace-1", "WS-1", &subitem)?;
            change.replace_attempt("workspace-1", "WS-1", &closed)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store.task_subitems("workspace-1", "WS-1").unwrap(),
        [subitem]
    );
    assert_eq!(
        store
            .task_comments("workspace-1", "WS-1", None, 10)
            .unwrap(),
        [comment]
    );
    assert_eq!(
        store.task_attempts("workspace-1", "WS-1").unwrap(),
        [closed]
    );
    assert_eq!(store.task_moves("workspace-1", "WS-1").unwrap(), [movement]);
}

#[test]
fn closing_an_unknown_attempt_fails_the_whole_mutation() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "task");
    let missing = Attempt {
        n: 1,
        pane_id: "pane-a".into(),
        harness: "harness".into(),
        model: "model".into(),
        start: "start".into(),
        end: Some("end".into()),
        result: Some(AttemptResult::Returned),
        summary: Some("closed".into()),
    };
    let error = match store.mutate("test", "missing-attempt", "missing-attempt", |change| {
        change.replace_attempt("workspace-1", "WS-1", &missing)?;
        Ok((json!({}), vec![]))
    }) {
        Ok(_) => panic!("closing an unknown attempt must fail"),
        Err(error) => error,
    };
    assert!(matches!(error, soyeht_store::Error::AttemptNotFound));
    assert!(
        store
            .task_attempts("workspace-1", "WS-1")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn task_writes_report_stale_versions_and_allocate_subitem_ids_in_the_transaction() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let original = task("WS-1", TaskPriority::Medium);
    insert(&mut store, &original, "task");
    let mut updated = original.clone();
    updated.version = 2;
    store
        .mutate("test", "first-update", "first-update", |change| {
            change.replace_task(&updated, 1)?;
            let first = change.allocate_subitem_id("workspace-1", "WS-1")?;
            assert_eq!(first, "WS-1-T01");
            change.insert_subitem(
                "workspace-1",
                "WS-1",
                &Subitem {
                    subitem_id: first,
                    task_id: "WS-1".into(),
                    text: "Plan".into(),
                    agent: None,
                    instructions: None,
                    state: SubitemState::Open,
                },
            )?;
            assert_eq!(
                change.allocate_subitem_id("workspace-1", "WS-1")?,
                "WS-1-T02"
            );
            Ok((json!({}), vec![]))
        })
        .unwrap();
    let error = match store.mutate("test", "stale-update", "stale-update", |change| {
        change.replace_task(&updated, 1)?;
        Ok((json!({}), vec![]))
    }) {
        Ok(_) => panic!("stale update must fail"),
        Err(error) => error,
    };
    assert!(matches!(error, soyeht_store::Error::TaskVersionConflict));
}

#[test]
fn comment_pages_use_an_exclusive_cursor_from_the_real_database() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "task");
    store
        .mutate("test", "comments", "comments", |change| {
            for (id, text) in [("first", "One"), ("second", "Two"), ("third", "Three")] {
                change.insert_comment(
                    "workspace-1",
                    "WS-1",
                    &Comment {
                        comment_id: id.into(),
                        task_id: "WS-1".into(),
                        author: "you".into(),
                        text: text.into(),
                        at: id.into(),
                    },
                )?;
            }
            Ok((json!({}), vec![]))
        })
        .unwrap();

    let first = store
        .task_comments_page("workspace-1", "WS-1", None, 2)
        .unwrap();
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.comment_id.as_str())
            .collect::<Vec<_>>(),
        ["third", "second"]
    );
    let second = store
        .task_comments_page("workspace-1", "WS-1", first.next_before, 2)
        .unwrap();
    assert_eq!(
        second
            .items
            .iter()
            .map(|item| item.comment_id.as_str())
            .collect::<Vec<_>>(),
        ["first"]
    );
    assert_eq!(second.next_before, None);
}

#[test]
fn loop_feedback_and_dispatch_checkpoints_survive_reopen() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "task");
    let config = LoopConfig {
        prompt: "Pull the next task".into(),
        interval: 30,
        max_rounds: 10,
        stop_when: LoopStopWhen::ThreeEmpty,
        enabled: true,
        rounds: Some(2),
        empty_results: Some(1),
    };
    let feedback = FeedbackState {
        fingerprint: "red-check".into(),
        delivery_count: 2,
        armed: true,
    };
    let checkpoint = DispatchCheckpoint {
        stage: "worktree_created".into(),
        worktree_path: Some("/tmp/tasks/WS-1".into()),
        branch: Some("task/ws-1".into()),
        pane_id: None,
    };
    store
        .mutate("test", "runtime", "runtime", |change| {
            change.save_loop_config("pane-a", &config, 4000)?;
            change.save_feedback("workspace-1", "WS-1", &feedback)?;
            change.save_dispatch_checkpoint("workspace-1", "WS-1", &checkpoint)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    drop(store);
    let mut store = profile.store();
    assert_eq!(
        store.loop_config("pane-a").unwrap(),
        Some((config.clone(), 4000))
    );
    assert!(store.due_loop_configs(3999).unwrap().is_empty());
    let due = store.due_loop_configs(4000).unwrap();
    assert_eq!(due, vec![("pane-a".into(), config.clone(), 4000)]);
    let snapshot = store.snapshot().unwrap();
    let loops = snapshot
        .sections
        .iter()
        .find(|(name, _)| name == "loops")
        .unwrap();
    assert_eq!(loops.1[0]["paneId"], "pane-a");
    assert_eq!(loops.1[0]["config"], json!(config));
    assert_eq!(
        store.feedback_state("workspace-1", "WS-1").unwrap(),
        Some(feedback)
    );
    assert_eq!(
        store.dispatch_checkpoint("workspace-1", "WS-1").unwrap(),
        Some(checkpoint)
    );
    let mut stopped = config;
    stopped.enabled = false;
    store
        .mutate("test", "stop-loop", "stop-loop", |change| {
            change.save_loop_config("pane-a", &stopped, 0)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.due_loop_configs(u64::MAX).unwrap().is_empty());
}

#[test]
fn typed_blocking_queries_and_retry_deadlines_use_the_real_database() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    let mut dependency = task("WS-1", TaskPriority::High);
    dependency.blocking = Some(soyeht_protocol::Blocking {
        kind: soyeht_protocol::BlockingKind::Dependency,
        reason: "waiting for WS-2".into(),
        dependency_task_id: Some("WS-2".into()),
        retry_count: None,
    });
    let mut external = task("WS-2", TaskPriority::Medium);
    external.blocking = Some(soyeht_protocol::Blocking {
        kind: soyeht_protocol::BlockingKind::ExternalError,
        reason: "service unavailable".into(),
        dependency_task_id: None,
        retry_count: Some(0),
    });
    store
        .mutate("test", "blocked", "blocked", |change| {
            change.insert_task(&dependency)?;
            change.insert_task(&external)?;
            change.schedule_task_retry("workspace-1", "WS-2", 2_000)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();

    assert_eq!(
        store.blocked_tasks().unwrap(),
        vec![dependency, external.clone()]
    );
    assert!(store.due_task_retries(1_999).unwrap().is_empty());
    assert_eq!(store.due_task_retries(2_000).unwrap(), vec![external]);

    store
        .mutate("test", "clear-retry", "clear-retry", |change| {
            change.clear_task_retry("workspace-1", "WS-2")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert!(store.due_task_retries(i64::MAX).unwrap().is_empty());
}

#[test]
fn review_inputs_survive_either_arrival_order_and_reject_stale_writes() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    for (task_id, reviewer) in [("WS-1", "reviewer-a"), ("WS-2", "reviewer-b")] {
        prepare_review(&mut store, task_id, reviewer);
    }

    replace_review(
        &mut store,
        "proposal-first",
        "WS-1",
        2,
        ReviewStage::PromptAccepted,
        Some("summary"),
        None,
    );
    replace_review(
        &mut store,
        "findings-second",
        "WS-1",
        3,
        ReviewStage::Classified,
        Some("summary"),
        Some(&["equivalent"]),
    );
    replace_review(
        &mut store,
        "findings-first",
        "WS-2",
        2,
        ReviewStage::Classified,
        None,
        Some(&["equivalent"]),
    );
    replace_review(
        &mut store,
        "proposal-second",
        "WS-2",
        3,
        ReviewStage::Classified,
        Some("summary"),
        Some(&["equivalent"]),
    );
    drop(store);

    let mut store = profile.store();
    for (task_id, reviewer) in [("WS-1", "reviewer-a"), ("WS-2", "reviewer-b")] {
        assert_complete_review(&store, task_id, reviewer);
    }
    let unresolved: Vec<UnresolvedTaskReview<ReviewPayload>> =
        store.unresolved_task_reviews().unwrap();
    assert_eq!(unresolved.len(), 2);
    assert_eq!(unresolved[0].task_id, "WS-1");
    assert_eq!(unresolved[1].task_id, "WS-2");

    let stale = store.mutate("test", "stale-review", "stale-review", |change| {
        change.replace_task_review(
            "workspace-1",
            "WS-1",
            1,
            2,
            ReviewStage::PaneOpened,
            &ReviewPayload {
                proposal: None,
                findings: None,
            },
        )?;
        Ok((json!({}), vec![]))
    });
    assert!(matches!(
        stale,
        Err(soyeht_store::Error::ReviewVersionConflict)
    ));
    let record: TaskReviewRecord<ReviewPayload> = store
        .task_review("workspace-1", "WS-1", 1)
        .unwrap()
        .unwrap();
    assert_eq!(record.version, 4);
    assert_eq!(record.stage, ReviewStage::Classified);
}

#[test]
fn migration_from_v7_preserves_tasks_and_adds_durable_reviews_and_retries() {
    let profile = Profile::new();
    let connection = rusqlite::Connection::open(profile.0.join("soyeht.db")).unwrap();
    for migration in [
        include_str!("../migrations/001-initial.sql"),
        include_str!("../migrations/002-client-state.sql"),
        include_str!("../migrations/003-english-values.sql"),
        include_str!("../migrations/004-agent-sessions.sql"),
        include_str!("../migrations/005-native-replay.sql"),
        include_str!("../migrations/006-agent-auth-snapshot.sql"),
        include_str!("../migrations/007-workspace-tasks.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection
        .execute_batch(
            "INSERT INTO metadata(key,value) VALUES ('db_id','v7-db'),('engine_generation','0');
             INSERT INTO workspaces(id,payload) VALUES ('workspace-1','{}');
             INSERT INTO tasks(workspace_id,task_id,state,priority_rank,version,payload)
               VALUES ('workspace-1','WS-1','in_review',1,4,'{}');
             INSERT INTO task_attempts(workspace_id,task_id,attempt,payload)
               VALUES ('workspace-1','WS-1',1,'{}');",
        )
        .unwrap();
    drop(connection);

    let store = profile.store();
    let version: u32 = store
        .connection()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let tasks: u32 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    let retries: u32 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM task_block_retries", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, soyeht_store::database::SCHEMA_VERSION);
    assert_eq!(tasks, 1);
    assert_eq!(retries, 0);
    assert_eq!(
        store
            .task_review::<ReviewPayload>("workspace-1", "WS-1", 1)
            .unwrap(),
        None
    );
}

#[test]
fn a_resolved_attempt_can_start_one_new_versioned_review_cycle() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    prepare_review(&mut store, "WS-1", "reviewer-a");
    replace_review(
        &mut store,
        "classify-cycle",
        "WS-1",
        2,
        ReviewStage::Classified,
        None,
        Some(&["equivalent"]),
    );
    replace_review(
        &mut store,
        "resolve-cycle",
        "WS-1",
        3,
        ReviewStage::Resolved,
        Some("summary"),
        Some(&["equivalent"]),
    );
    let payload = ReviewPayload {
        proposal: None,
        findings: None,
    };
    store
        .mutate("test", "restart-cycle", "restart-cycle", |change| {
            change.restart_task_review(RestartTaskReview {
                workspace_id: "workspace-1",
                task_id: "WS-1",
                attempt: 1,
                expected_version: 4,
                reviewer_pane_id: "reviewer-b",
                stage: ReviewStage::PromptAccepted,
                payload: &payload,
            })?;
            Ok((json!({}), vec![]))
        })
        .unwrap();

    let restarted: TaskReviewRecord<ReviewPayload> = store
        .task_review("workspace-1", "WS-1", 1)
        .unwrap()
        .unwrap();
    assert_eq!(restarted.version, 5);
    assert_eq!(restarted.stage, ReviewStage::PromptAccepted);
    assert_eq!(restarted.reviewer_pane_id, "reviewer-b");
    assert_eq!(restarted.payload, payload);
}

#[test]
fn dispatch_journal_lists_updates_and_removes_internal_steps() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(
        &mut store,
        &task("WS-1", TaskPriority::Medium),
        "journal-task",
    );
    let checkpoint = DispatchCheckpoint {
        stage: "reserved".into(),
        worktree_path: None,
        branch: Some("task/ws-1".into()),
        pane_id: Some("pane-1".into()),
    };
    store
        .record_dispatch_checkpoint("workspace-1", "WS-1", &checkpoint)
        .unwrap();
    assert_eq!(
        store.dispatch_checkpoints().unwrap(),
        vec![("workspace-1".into(), "WS-1".into(), checkpoint.clone())]
    );

    let mut advanced = checkpoint;
    advanced.stage = "worktree_created".into();
    advanced.worktree_path = Some("/tmp/worktree".into());
    store
        .record_dispatch_checkpoint("workspace-1", "WS-1", &advanced)
        .unwrap();
    assert_eq!(
        store.dispatch_checkpoint("workspace-1", "WS-1").unwrap(),
        Some(advanced)
    );
}

#[test]
fn one_real_database_transaction_reserves_a_task_only_once() {
    let profile = Arc::new(Profile::new());
    let mut seed_store = profile.store();
    seed(&mut seed_store);
    insert(&mut seed_store, &task("WS-1", TaskPriority::Medium), "task");
    drop(seed_store);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles = ["pane-a", "pane-b"].map(|pane_id| {
        let profile = Arc::clone(&profile);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let mut store = profile.store();
            barrier.wait();
            store
                .mutate("race", pane_id, pane_id, |change| {
                    let claimed = change.reserve_next_task(
                        "workspace-1",
                        pane_id,
                        "harness",
                        "model",
                        "start",
                    )?;
                    Ok((json!(claimed.as_ref().map(|item| &item.task_id)), vec![]))
                })
                .unwrap()
                .result
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.iter().filter(|value| !value.is_null()).count(), 1);
    let store = profile.store();
    let claimed = store.task("workspace-1", "WS-1").unwrap().unwrap();
    assert_eq!(claimed.state, TaskState::InProgress);
    assert!(claimed.owner_pane_id.is_some());
    assert_eq!(store.task_attempts("workspace-1", "WS-1").unwrap().len(), 1);
    assert!(
        store
            .active_task_for_pane(claimed.owner_pane_id.as_deref().unwrap())
            .unwrap()
            .is_some()
    );
}

#[test]
fn explicit_dispatch_reserves_once_and_records_the_selected_preset() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "task");
    let preset = TaskExecutor {
        workspace_default: false,
        harness: Some("harness-a".into()),
        model: Some("model-a".into()),
        effort: Some("high".into()),
        permission_mode: Some("ask".into()),
    };
    let claimed = store
        .mutate("test", "dispatch", "dispatch", |change| {
            let claimed = change.reserve_task(
                "workspace-1",
                "WS-1",
                &preset,
                &TaskReservation {
                    pane_id: "pane-a",
                    harness: "harness-a",
                    model: "model-a",
                    started_at: "start",
                },
            )?;
            Ok((json!(claimed), vec![]))
        })
        .unwrap();
    assert_eq!(claimed.result["ownerPaneId"], "pane-a");
    assert_eq!(claimed.result["executor"]["harness"], "harness-a");
    let error = match store.mutate("test", "second", "second", |change| {
        change.reserve_task(
            "workspace-1",
            "WS-1",
            &preset,
            &TaskReservation {
                pane_id: "pane-b",
                harness: "harness-a",
                model: "model-a",
                started_at: "later",
            },
        )?;
        Ok((json!({}), vec![]))
    }) {
        Ok(_) => panic!("a reserved task must reject a second dispatch"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        soyeht_store::Error::TaskReservationConflict
    ));
    assert_eq!(store.task_attempts("workspace-1", "WS-1").unwrap().len(), 1);
}

#[test]
fn snapshot_contains_the_task_board_from_the_same_read_transaction() {
    let profile = Profile::new();
    let mut store = profile.store();
    seed(&mut store);
    insert(&mut store, &task("WS-1", TaskPriority::Medium), "task");
    let snapshot = store.snapshot().unwrap();
    let tasks = snapshot
        .sections
        .iter()
        .find(|(name, _)| name == "tasks")
        .unwrap();
    assert_eq!(tasks.1.as_array().unwrap().len(), 1);
}
