#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use soyeht_protocol::{
    Attempt, AttemptResult, Comment, LoopConfig, LoopStopWhen, Pane, PaneMode, PaneState,
    StateSource, Subitem, SubitemState, Task, TaskMove, TaskPriority, TaskState, Workspace,
};
use soyeht_store::{
    database::Store,
    domain::PaneRecord,
    tasks::{DispatchCheckpoint, FeedbackState},
};
use std::{fs, path::PathBuf, sync::Arc, thread};
use uuid::Uuid;

struct Profile(PathBuf);

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
            pid: None,
            stream_id: None,
            agent: None,
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
    assert_eq!(store.loop_config("pane-a").unwrap(), Some((config, 4000)));
    assert_eq!(
        store.feedback_state("workspace-1", "WS-1").unwrap(),
        Some(feedback)
    );
    assert_eq!(
        store.dispatch_checkpoint("workspace-1", "WS-1").unwrap(),
        Some(checkpoint)
    );
    store
        .mutate("test", "clear", "clear", |change| {
            change.clear_dispatch_checkpoint("workspace-1", "WS-1")?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
    assert_eq!(
        store.dispatch_checkpoint("workspace-1", "WS-1").unwrap(),
        None
    );
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
    store
        .remove_dispatch_checkpoint("workspace-1", "WS-1")
        .unwrap();
    assert!(store.dispatch_checkpoints().unwrap().is_empty());
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
