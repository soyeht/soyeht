use super::*;
use crate::{
    tasks::rules::tests::task_fixture,
    test_panes::{core_with_workspace, insert_agent_pane},
};
use soyeht_protocol::{Comment, TaskMove, TaskState, limits};

const WORKSPACE: &str = "workspace-1";

fn comment(task: &str, n: usize, text: String) -> Comment {
    Comment {
        comment_id: format!("c{n}"),
        task_id: task.into(),
        author: "you".into(),
        text,
        at: "2026-09-11T00:00:00Z".into(),
    }
}

fn opened(pane: Option<&str>) -> DispatchCheckpoint {
    DispatchCheckpoint {
        stage: "pane_opened".into(),
        worktree_path: Some("/work/task".into()),
        branch: Some("task/work".into()),
        pane_id: pane.map(Into::into),
    }
}

/// A core with the tasks `A-1` and `A-2`, each claimed by an idle agent pane and with its
/// dispatch at `pane_opened`, after the task `A-0`, whose checkpoint names no pane and so
/// cannot advance. `comments` belong to `A-1`.
fn core_with_claimed_tasks(comments: Vec<Comment>) -> (Core, std::path::PathBuf) {
    let (mut core, directory) = core_with_workspace(WORKSPACE);
    for pane in ["pane-1", "pane-2"] {
        insert_agent_pane(&mut core, &directory, pane, WORKSPACE);
    }
    let rows = [
        ("A-0", None),
        ("A-1", Some("pane-1")),
        ("A-2", Some("pane-2")),
    ];
    StorePort::mutate(&mut core.store, "test", "tasks", "seed-tasks", |change| {
        for (id, pane) in rows {
            let mut task = task_fixture(TaskState::InProgress);
            task.workspace_id = WORKSPACE.into();
            task.task_id = id.into();
            task.owner_pane_id = pane.map(Into::into);
            change.insert_task(&task)?;
            change.insert_move(
                WORKSPACE,
                id,
                &TaskMove {
                    actor: "you".into(),
                    from: TaskState::Todo,
                    to: TaskState::InProgress,
                    reason: "claimed".into(),
                    at: "2026-09-11T00:00:00Z".into(),
                },
            )?;
            change.save_dispatch_checkpoint(WORKSPACE, id, &opened(pane))?;
        }
        for item in &comments {
            change.insert_comment(WORKSPACE, "A-1", item)?;
        }
        Ok((json!({}), Vec::new()))
    })
    .unwrap();
    (core, directory)
}

/// A core with the task `A-1`, in progress with no owner and a dispatch checkpoint that names no
/// pane, so its dispatch cannot advance.
fn core_with_waiting_dispatch() -> (Core, std::path::PathBuf) {
    let (mut core, directory) = core_with_workspace(WORKSPACE);
    StorePort::mutate(
        &mut core.store,
        "test",
        "tasks",
        "seed-waiting-dispatch",
        |change| {
            let mut task = task_fixture(TaskState::InProgress);
            task.workspace_id = WORKSPACE.into();
            task.task_id = "A-1".into();
            task.owner_pane_id = None;
            change.insert_task(&task)?;
            // Its checkpoint names no pane, so the dispatch cannot advance.
            change.save_dispatch_checkpoint(WORKSPACE, "A-1", &opened(None))?;
            Ok((json!({}), Vec::new()))
        },
    )
    .unwrap();
    (core, directory)
}

fn stage(core: &Core, task: &str) -> String {
    core.store
        .dispatch_checkpoint(WORKSPACE, task)
        .unwrap()
        .unwrap()
        .stage
}

fn first_text(core: &Core, pane: &str) -> String {
    let queue = &core.agents.running[pane].state.queue;
    assert_eq!(queue.len(), 1, "{pane} holds its first prompt");
    queue[0].text.clone()
}

#[test]
fn a_task_with_more_comments_than_a_prompt_holds_still_gets_its_first_prompt() {
    let many = (0..210)
        .map(|n| comment("A-1", n, format!("note {n} {}", "x".repeat(400))))
        .collect();
    let (mut core, directory) = core_with_claimed_tasks(many);

    core.advance_task_dispatches().unwrap();

    assert_eq!(stage(&core, "A-1"), "prompt_accepted");
    let text = first_text(&core, "pane-1");
    assert!(text.len() <= limits::PROMPT_BYTES);
    assert!(text.contains("earlier comments omitted"));
    assert!(text.contains("note 209 "), "the newest comment is kept");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_comment_longer_than_a_prompt_is_cut_and_the_task_is_dispatched() {
    let (mut core, directory) = core_with_claimed_tasks(vec![comment(
        "A-1",
        1,
        "y".repeat(limits::PROMPT_BYTES + 1),
    )]);

    core.advance_task_dispatches().unwrap();

    assert_eq!(stage(&core, "A-1"), "prompt_accepted");
    assert!(first_text(&core, "pane-1").len() <= limits::PROMPT_BYTES);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_dispatch_row_that_cannot_advance_does_not_stop_the_rows_after_it() {
    let (mut core, directory) = core_with_claimed_tasks(Vec::new());

    // `A-0` sorts first and its checkpoint names no pane.
    core.advance_task_dispatches().unwrap();

    assert_eq!(stage(&core, "A-0"), "pane_opened");
    assert_eq!(stage(&core, "A-1"), "prompt_accepted");
    assert_eq!(stage(&core, "A-2"), "prompt_accepted");
    assert!(first_text(&core, "pane-2").contains("# Do the work"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_dispatch_row_that_waits_rests_for_the_retry_delay() {
    let (mut core, directory) = core_with_waiting_dispatch();

    core.advance_task_dispatches().unwrap();

    crate::tasks::assert_poll_rests_for_retry(&core);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_store_write_ends_the_rest_of_a_task_poll_that_is_waiting() {
    let (mut core, directory) = core_with_waiting_dispatch();

    core.advance_task_dispatches().unwrap();
    crate::tasks::assert_poll_rests_for_retry(&core);

    // Any write ends the rest: the poll asks again now, not at its retry delay.
    StorePort::mutate(
        &mut core.store,
        "test",
        "tasks-write",
        "write-another-row",
        |change| {
            let mut task = task_fixture(TaskState::Todo);
            task.workspace_id = WORKSPACE.into();
            task.task_id = "A-2".into();
            task.owner_pane_id = None;
            change.insert_task(&task)?;
            Ok((json!({}), Vec::new()))
        },
    )
    .unwrap();

    assert_eq!(
        core.store.next_poll_wait(core.now_ms()),
        Some(std::time::Duration::ZERO),
        "a store write ends the rest of the waiting poll"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_caller_gets_the_error_of_its_own_row_and_not_the_error_of_another() {
    let (mut core, directory) = core_with_claimed_tasks(Vec::new());

    // `A-0` cannot advance. A caller of `A-1` is not told, and `A-2` still advances.
    core.advance_dispatches_for(WORKSPACE, "A-1").unwrap();
    assert_eq!(stage(&core, "A-1"), "prompt_accepted");
    assert_eq!(stage(&core, "A-2"), "prompt_accepted");
    assert_eq!(stage(&core, "A-0"), "pane_opened");

    // The caller of the failing row gets its failure, as before.
    let error = core.advance_dispatches_for(WORKSPACE, "A-0").unwrap_err();
    assert_eq!(
        errors::kind(&error),
        soyeht_protocol::ErrorKind::InternalError
    );
    std::fs::remove_dir_all(directory).unwrap();
}
