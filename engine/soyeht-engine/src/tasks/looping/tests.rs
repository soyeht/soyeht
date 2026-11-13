use super::*;
use crate::{
    ports::testing::FixedClock, supervisor_link::tests::short_directory,
    test_panes::insert_agent_pane,
};
use soyeht_protocol::{LoopSetParams, Workspace, limits};
use std::{path::Path, sync::Arc};

const NOW_MS: i64 = 1_000_000;

fn loop_config(prompt: String) -> LoopConfig {
    LoopConfig {
        prompt,
        interval: 60,
        max_rounds: 0,
        stop_when: LoopStopWhen::ExplicitOff,
        enabled: true,
        rounds: Some(0),
        empty_results: Some(0),
    }
}

/// A core with the idle agent panes `pane-a` and `pane-b`, each in a workspace of its own, and
/// `pane-a` ahead of `pane-b` in the order the loops are read.
fn core_with_two_loops(first: LoopConfig, second: LoopConfig) -> (Core, std::path::PathBuf) {
    let directory = short_directory();
    let mut core = Core::open(&directory).unwrap();
    core.clock = Arc::new(FixedClock(NOW_MS));
    core.background = crate::offload::Background::inline();
    for (pane, workspace) in [("pane-a", "workspace-a"), ("pane-b", "workspace-b")] {
        let record = Workspace {
            workspace_id: workspace.into(),
            name: workspace.into(),
            path: directory.to_string_lossy().into(),
            layout: json!({}),
        };
        StorePort::mutate(&mut core.store, "test", workspace, "seed", |change| {
            change.insert_workspace(&record)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
        insert_agent_pane(&mut core, &directory, pane, workspace);
    }
    StorePort::mutate(&mut core.store, "test", "loops", "seed-loops", |change| {
        change.save_loop_config("pane-a", &first, 100)?;
        change.save_loop_config("pane-b", &second, 200)?;
        Ok((json!({}), Vec::new()))
    })
    .unwrap();
    (core, directory)
}

fn queued(core: &Core, pane: &str) -> Vec<String> {
    core.agents.running[pane]
        .state
        .queue
        .iter()
        .map(|item| item.prompt_id.clone())
        .collect()
}

fn remove(directory: &Path) {
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_loop_row_that_cannot_dispatch_does_not_stop_the_loops_after_it() {
    // A row stored before the prompt length was checked: the first in the order, never able
    // to enter a queue.
    let oversized = loop_config("x".repeat(limits::PROMPT_BYTES + 1));
    let (mut core, directory) = core_with_two_loops(oversized, loop_config("go on".into()));

    core.advance_task_loops().unwrap();

    assert!(queued(&core, "pane-a").is_empty());
    let queue = queued(&core, "pane-b");
    assert_eq!(queue.len(), 1);
    assert!(queue[0].starts_with(LOOP_PROMPT_PREFIX));
    let (_, held_until) = core.store.loop_config("pane-a").unwrap().unwrap();
    assert!(
        held_until > NOW_MS as u64,
        "the failed row waits for its next interval instead of leading the order every tick"
    );

    core.advance_task_loops().unwrap();
    assert!(queued(&core, "pane-a").is_empty());
    assert_eq!(
        queued(&core, "pane-b"),
        queue,
        "nothing is dispatched twice"
    );
    remove(&directory);
}

#[test]
fn a_loop_prompt_is_bounded_by_the_prompt_limit() {
    let params = |prompt: String, enabled| LoopSetParams {
        pane_id: "pane-a".into(),
        prompt,
        interval: 60,
        max_rounds: 0,
        stop_when: LoopStopWhen::ExplicitOff,
        enabled,
        idempotency_key: "key".into(),
    };
    let limit = "x".repeat(limits::PROMPT_BYTES);
    assert!(new_loop_config(&params(limit.clone(), true)).is_ok());
    for enabled in [true, false] {
        let refused = new_loop_config(&params(format!("{limit}x"), enabled)).unwrap_err();
        assert_eq!(
            errors::kind(&refused),
            soyeht_protocol::ErrorKind::InvalidParams
        );
    }
}

#[test]
fn holding_a_loop_back_keeps_the_saved_round_and_does_not_enable_a_finished_loop() {
    let counted = LoopConfig {
        rounds: Some(5),
        ..loop_config("go on".into())
    };
    let finished = LoopConfig {
        enabled: false,
        rounds: Some(7),
        ..loop_config("go on".into())
    };
    let (mut core, directory) = core_with_two_loops(counted, finished.clone());

    core.hold_task_loop("pane-a", NOW_MS as u64).unwrap();
    let (kept, held_until) = core.store.loop_config("pane-a").unwrap().unwrap();
    assert_eq!(kept.rounds, Some(5), "the round already saved stays");
    assert!(kept.enabled);
    assert!(held_until > NOW_MS as u64);

    core.hold_task_loop("pane-b", NOW_MS as u64).unwrap();
    assert_eq!(
        core.store.loop_config("pane-b").unwrap(),
        Some((finished, 200)),
        "a loop that has ended is left as it is"
    );
    remove(&directory);
}
