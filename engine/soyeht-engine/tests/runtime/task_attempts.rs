use super::*;

#[test]
fn active_executor_blocks_requests_review_and_completes_one_durable_attempt() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Complete lifecycle", "reserve-complete");
    let blocked = lifecycle.executor_request(
        "task.block",
        &task,
        json!({"kind":"missing_information","reason":"Need an answer"}),
        "block",
    );
    assert_eq!(
        blocked["result"]["task"]["blocking"]["kind"],
        "missing_information"
    );
    let attention = lifecycle.human.section("attention");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["paneId"], lifecycle.pane);
    assert!(
        attention[0]["message"]
            .as_str()
            .unwrap()
            .contains("Need an answer")
    );
    lifecycle.human.call(
        "task.unblock",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":task.task_id,
            "reason":"Answer supplied","idempotencyKey":"unblock"
        }),
    );
    assert!(lifecycle.human.section("attention").is_empty());
    let added = lifecycle.executor_request(
        "task.subitem.add",
        &task,
        json!({"text":"Check the result"}),
        "add-step",
    );
    let refused = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "review-too-early",
    );
    assert_eq!(refused["error"]["data"]["kind"], "transition_refused");
    lifecycle.executor_request(
        "task.subitem.done",
        &task,
        json!({"subitemId":added["result"]["subitem"]["subitemId"]}),
        "finish-step",
    );
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-review",
    );
    assert_eq!(review["result"]["task"]["state"], "in_review");
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    let mut stale = review_task.clone();
    stale.current_attempt = 0;
    let refused = lifecycle.executor_request(
        "task.complete",
        &stale,
        json!({"summary":"Done","contractChanges":[],"outOfScopeEdits":[]}),
        "stale-complete",
    );
    assert_eq!(refused["error"]["data"]["kind"], "stale_attempt");
    let fields = json!({"summary":"Done","contractChanges":[],"outOfScopeEdits":[]});
    let completed =
        lifecycle.executor_request("task.complete", &review_task, fields.clone(), "complete");
    assert_eq!(completed["result"]["task"]["state"], "in_review");
    let shown = lifecycle.wait_task_state(&task.task_id, "completed");
    assert!(shown["task"]["ownerPaneId"].is_null());
    let repeated = lifecycle.executor_request("task.complete", &review_task, fields, "complete");
    assert_eq!(repeated["result"], completed["result"]);
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let review: TaskReviewRecord<Value> = store
        .task_review(&lifecycle.workspace, &task.task_id, task.current_attempt)
        .unwrap()
        .unwrap();
    assert_eq!(review.stage, ReviewStage::Resolved);
    assert_eq!(review.payload["decision"]["approved"], true);
    let reviewer = lifecycle.human.wait_state(&review.reviewer_pane_id, "idle");
    let session = reviewer["agent"]["sessionId"].as_str().unwrap();
    assert!(
        lifecycle.rig.native_session(session)["mcp_servers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Completed)
    );
    assert_eq!(attempts[0].summary.as_deref(), Some("Done"));
    assert!(attempts[0].end.is_some());
    assert_eq!(
        store
            .task_moves(&lifecycle.workspace, &task.task_id)
            .unwrap()
            .len(),
        2
    );
}

fn replace_task_state(rig: &Rig, workspace: &str, task_id: &str, state: &str, key: &str) {
    let mut store = Store::open(&rig.path).unwrap();
    let mut task = store.task(workspace, task_id).unwrap().unwrap();
    task.state = serde_json::from_value(json!(state)).unwrap();
    task.version += 1;
    task.phrase = state.replace('_', " ");
    store
        .mutate("test", key, key, |change| {
            change.replace_task(&task, task.version - 1)?;
            Ok((json!({}), vec![]))
        })
        .unwrap();
}

fn completed_dependency_returns_task(lifecycle: &mut TaskLifecycleRig) -> Task {
    let task = lifecycle.create_and_reserve("Dependency target", "reserve-dependency-target");
    let dependency = lifecycle.human.call(
        "task.create",
        json!({
            "workspaceId":lifecycle.workspace,"title":"Dependency",
            "mustNotChange":"Keep behavior","idempotencyKey":"create-dependency"
        }),
    )["task"]
        .clone();
    lifecycle.executor_request(
        "task.block",
        &task,
        json!({"kind":"dependency","reason":"waiting","dependencyTaskId":dependency["taskId"]}),
        "block-dependency",
    );
    replace_task_state(
        &lifecycle.rig,
        &lifecycle.workspace,
        dependency["taskId"].as_str().unwrap(),
        "completed",
        "complete-dependency",
    );
    let returned = lifecycle.wait_task_state(&task.task_id, "todo");
    assert!(returned["task"]["blocking"].is_null());
    assert!(returned["task"]["ownerPaneId"].is_null());
    reserve_task(
        &lifecycle.rig,
        &lifecycle.workspace,
        &lifecycle.pane,
        "reserve-cancelled-dependency",
    )
}

fn cancelled_dependency_requires_a_decision(lifecycle: &mut TaskLifecycleRig, active: &Task) {
    let cancelled_dependency = lifecycle.human.call(
        "task.create",
        json!({
            "workspaceId":lifecycle.workspace,"title":"Cancelled dependency",
            "mustNotChange":"Keep behavior","idempotencyKey":"create-cancelled-dependency"
        }),
    )["task"]
        .clone();
    lifecycle.executor_request(
        "task.block",
        active,
        json!({"kind":"dependency","reason":"waiting","dependencyTaskId":cancelled_dependency["taskId"]}),
        "block-cancelled-dependency",
    );
    replace_task_state(
        &lifecycle.rig,
        &lifecycle.workspace,
        cancelled_dependency["taskId"].as_str().unwrap(),
        "cancelled",
        "cancel-dependency",
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let shown = lifecycle.human.call(
            "task.show",
            json!({"workspaceId":lifecycle.workspace,"taskId":active.task_id}),
        );
        if shown["task"]["phrase"] == "blocked: dependency cancelled" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cancelled dependency was not reported: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    lifecycle.human.call(
        "task.unblock",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":active.task_id,
            "reason":"Proceed without dependency","idempotencyKey":"unblock-cancelled-dependency"
        }),
    );
}

fn wait_for_retry(lifecycle: &mut TaskLifecycleRig, active: &Task, count: u32) {
    lifecycle
        .rig
        .database_probe()
        .make_task_retry_due(&lifecycle.workspace, &active.task_id)
        .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let shown = lifecycle.human.call(
            "task.show",
            json!({"workspaceId":lifecycle.workspace,"taskId":active.task_id}),
        );
        if shown["task"]["blocking"]["retryCount"] == count {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "retry {count} did not run: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn external_error_exhausts_two_durable_retries(lifecycle: &mut TaskLifecycleRig, active: &Task) {
    lifecycle.executor_request(
        "task.block",
        active,
        json!({"kind":"external_error","reason":"service unavailable"}),
        "block-external",
    );
    for count in [1, 2] {
        wait_for_retry(lifecycle, active, count);
        lifecycle.executor_request(
            "task.block",
            active,
            json!({"kind":"external_error","reason":"service unavailable"}),
            &format!("external-still-blocked-{count}"),
        );
    }
    wait_for_external_escalation(lifecycle, active);
    assert_eq!(lifecycle.human.section("attention").len(), 1);
}

fn wait_for_external_escalation(lifecycle: &mut TaskLifecycleRig, active: &Task) {
    lifecycle
        .rig
        .database_probe()
        .make_task_retry_due(&lifecycle.workspace, &active.task_id)
        .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let shown = lifecycle.human.call(
            "task.show",
            json!({"workspaceId":lifecycle.workspace,"taskId":active.task_id}),
        );
        if shown["task"]["blocking"]["kind"] == "missing_information" {
            assert!(
                shown["task"]["phrase"]
                    .as_str()
                    .unwrap()
                    .starts_with("blocked:")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "external block did not escalate: {shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn typed_blocks_advance_from_real_engine_ticks_and_durable_deadlines() {
    let mut lifecycle = TaskLifecycleRig::new();
    let active = completed_dependency_returns_task(&mut lifecycle);
    cancelled_dependency_requires_a_decision(&mut lifecycle, &active);
    external_error_exhausts_two_durable_retries(&mut lifecycle, &active);
}

#[test]
fn undeclared_contract_drift_returns_the_same_attempt_with_a_review_prompt() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Return drift", "reserve-drift");
    fs::write(
        lifecycle.repository.join("src/lib.rs"),
        "pub fn calculate(value: Option<u32>) -> u32 {\n    value.unwrap_or_default()\n}\n",
    )
    .unwrap();
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-drift-review",
    );
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    lifecycle.executor_request(
        "task.complete",
        &review_task,
        json!({"summary":"Candidate","contractChanges":[],"outOfScopeEdits":[]}),
        "propose-drift-completion",
    );

    let shown = lifecycle.wait_task_state(&task.task_id, "in_progress");
    assert_eq!(shown["task"]["ownerPaneId"], lifecycle.pane);
    assert_eq!(shown["task"]["reviewBadge"], "your_turn");
    assert_eq!(shown["task"]["phrase"], "review requested changes");
    assert_eq!(shown["task"]["summary"], "Candidate");
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert!(attempts[0].end.is_none());
    assert!(attempts[0].result.is_none());
    let comments = store
        .task_comments(&lifecycle.workspace, &task.task_id, None, 10)
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].text.contains("silent_fallback"));
    assert!(comments[0].text.contains("returned:"));
    let review: TaskReviewRecord<Value> = store
        .task_review(&lifecycle.workspace, &task.task_id, task.current_attempt)
        .unwrap()
        .unwrap();
    assert_eq!(review.stage, ReviewStage::Resolved);
    assert_eq!(review.payload["decision"]["approved"], false);
    let transcript_or_queue = soyeht_store::agents::queue(&store, &lifecycle.pane)
        .unwrap()
        .iter()
        .any(|item| item.text.contains("silent_fallback"))
        || soyeht_store::agents::transcript(&store, &lifecycle.pane, None, 200)
            .unwrap()
            .items
            .iter()
            .any(|item| item.text.contains("silent_fallback"));
    assert!(transcript_or_queue);
    drop(store);

    lifecycle.human.wait_state(&lifecycle.pane, "idle");
    fs::write(
        lifecycle.repository.join("src/lib.rs"),
        "pub fn calculate(value: u32) -> u32 {\n    value + 3\n}\n",
    )
    .unwrap();
    let returned: Task = serde_json::from_value(shown["task"].clone()).unwrap();
    let second = lifecycle.executor_request(
        "task.request_review",
        &returned,
        json!({"reason":"Corrected"}),
        "request-corrected-review",
    );
    let second: Task = serde_json::from_value(second["result"]["task"].clone()).unwrap();
    lifecycle.executor_request(
        "task.complete",
        &second,
        json!({"summary":"Corrected","contractChanges":[],"outOfScopeEdits":[]}),
        "propose-corrected-completion",
    );
    let completed = lifecycle.wait_task_state(&task.task_id, "completed");
    assert_eq!(completed["task"]["summary"], "Corrected");
    assert!(lifecycle.review(&task).version >= 7);
}

#[test]
fn review_proposal_and_reviewer_turn_survive_an_engine_restart() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Restart review", "reserve-restart-review");
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-restart-review",
    );
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    lifecycle.executor_request(
        "task.complete",
        &review_task,
        json!({"summary":"Restart-safe","contractChanges":[],"outOfScopeEdits":[]}),
        "propose-before-restart",
    );
    let durable = lifecycle.review(&task);
    assert_eq!(durable.payload["completion"]["summary"], "Restart-safe");
    lifecycle.rig.kill_engine();
    lifecycle.rig.start();
    lifecycle.human = lifecycle.rig.client();

    let shown = lifecycle.wait_task_state(&task.task_id, "completed");
    assert_eq!(shown["task"]["summary"], "Restart-safe");
    let review = lifecycle.wait_review_stage(&task, ReviewStage::Resolved);
    assert_eq!(review.payload["decision"]["approved"], true);
    assert_eq!(
        Store::open(&lifecycle.rig.path)
            .unwrap()
            .task_attempts(&lifecycle.workspace, &task.task_id)
            .unwrap()[0]
            .result,
        Some(soyeht_protocol::AttemptResult::Completed)
    );
}

#[test]
fn reviewer_findings_resolve_after_completion_arrives_through_a_restarted_engine() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Findings first", "reserve-findings-first");
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "request-findings-first",
    );
    let review_task: Task = serde_json::from_value(review["result"]["task"].clone()).unwrap();
    let classified = lifecycle.wait_review_stage(&task, ReviewStage::Classified);
    assert_eq!(classified.payload["completion"], Value::Null);
    assert_eq!(classified.payload["findings"][0]["label"], "equivalent");

    lifecycle.rig.kill_engine();
    lifecycle.rig.start();
    lifecycle.human = lifecycle.rig.client();
    lifecycle.reconnect_executor();
    let proposed = lifecycle.executor_request(
        "task.complete",
        &review_task,
        json!({"summary":"Findings survived","contractChanges":[],"outOfScopeEdits":[]}),
        "complete-after-findings-restart",
    );
    assert_eq!(proposed["result"]["task"]["state"], "in_review");

    let shown = lifecycle.wait_task_state(&task.task_id, "completed");
    assert_eq!(shown["task"]["summary"], "Findings survived");
    let resolved = lifecycle.wait_review_stage(&task, ReviewStage::Resolved);
    assert_eq!(resolved.payload["decision"]["approved"], true);
}

#[test]
fn executor_release_is_idempotent_and_closes_the_attempt_as_returned() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Return lifecycle", "reserve-return");
    let fields = json!({"reason":"Needs another pass"});
    let released =
        lifecycle.executor_request("task.release", &task, fields.clone(), "release-task");
    assert_eq!(released["result"]["task"]["state"], "todo");
    assert!(released["result"]["task"]["ownerPaneId"].is_null());
    let repeated = lifecycle.executor_request("task.release", &task, fields, "release-task");
    assert_eq!(repeated["result"], released["result"]);
    let store = Store::open(&lifecycle.rig.path).unwrap();
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    assert_eq!(attempts[0].summary.as_deref(), Some("Needs another pass"));
    assert_eq!(
        store
            .task_moves(&lifecycle.workspace, &task.task_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn dying_executor_returns_active_work_and_records_one_system_comment() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Recover after death", "reserve-death");
    lifecycle.human.call(
        "pane.prompt",
        json!({"paneId":lifecycle.pane,"text":"die","idempotencyKey":"die-turn"}),
    );
    lifecycle.human.wait_state(&lifecycle.pane, "dead");

    let store = Store::open(&lifecycle.rig.path).unwrap();
    let returned = store
        .task(&lifecycle.workspace, &task.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(returned.state, soyeht_protocol::TaskState::Todo);
    assert_eq!(returned.owner_pane_id, None);
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    let comments = store
        .task_comments(&lifecycle.workspace, &task.task_id, None, 10)
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "system");
    assert_eq!(comments[0].text, "returned: the pane closed");
}

#[test]
fn closing_a_review_pane_unbinds_it_without_moving_the_card_back() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Preserve review", "reserve-review-close");
    let review = lifecycle.executor_request(
        "task.request_review",
        &task,
        json!({"reason":"Ready"}),
        "review-before-close",
    );
    assert_eq!(review["result"]["task"]["state"], "in_review");
    lifecycle.human.call(
        "pane.close",
        json!({"paneId":lifecycle.pane,"idempotencyKey":"close-review-pane"}),
    );

    let store = Store::open(&lifecycle.rig.path).unwrap();
    let waiting = store
        .task(&lifecycle.workspace, &task.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(waiting.state, soyeht_protocol::TaskState::InReview);
    assert_eq!(waiting.owner_pane_id, None);
    assert_eq!(
        waiting.review_badge,
        Some(soyeht_protocol::TaskReviewBadge::YourTurn)
    );
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Returned)
    );
    assert_eq!(
        store
            .task_moves(&lifecycle.workspace, &task.task_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn human_cancellation_stops_the_turn_and_closes_the_attempt_before_the_move() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Cancel active work", "reserve-cancel");
    lifecycle.human.call(
        "pane.prompt",
        json!({"paneId":lifecycle.pane,"text":"silent","idempotencyKey":"active-turn"}),
    );
    lifecycle.human.wait_state(&lifecycle.pane, "working");
    let params = json!({"workspaceId":lifecycle.workspace,"taskId":task.task_id,
        "state":"cancelled","reason":"Stopped by a person","idempotencyKey":"move-cancelled"});
    let cancelled = lifecycle.human.call("task.move", params.clone());
    assert_eq!(cancelled["task"]["state"], "cancelled");
    assert!(cancelled["task"]["ownerPaneId"].is_null());
    assert_eq!(lifecycle.human.call("task.move", params), cancelled);
    lifecycle.human.wait_state(&lifecycle.pane, "idle");

    let store = Store::open(&lifecycle.rig.path).unwrap();
    let attempts = store
        .task_attempts(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].result,
        Some(soyeht_protocol::AttemptResult::Cancelled)
    );
    assert_eq!(attempts[0].summary.as_deref(), Some("Stopped by a person"));
    let history = store
        .task_moves(&lifecycle.workspace, &task.task_id)
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].actor, "you");
    assert_eq!(history[0].from, soyeht_protocol::TaskState::InProgress);
    assert_eq!(history[0].to, soyeht_protocol::TaskState::Cancelled);
    assert_eq!(history[0].reason, "Stopped by a person");
}

#[test]
fn human_drag_table_refuses_reserved_targets_and_records_accepted_moves() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Moves","path":rig.path,"idempotencyKey":"move-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = client.call(
        "task.create",
        json!({"workspaceId":workspace,"title":"Move card","mustNotChange":"Keep history",
            "idempotencyKey":"move-task"}),
    )["task"]
        .clone();
    for (state, key) in [("in_progress", "move-active"), ("completed", "move-done")] {
        let refused = client.request(
            "task.move",
            json!({"workspaceId":workspace,"taskId":task["taskId"],"state":state,
                "reason":"Invalid drag","idempotencyKey":key}),
        );
        assert_eq!(refused["error"]["data"]["kind"], "transition_refused");
    }
    let cancelled = client.call(
        "task.move",
        json!({"workspaceId":workspace,"taskId":task["taskId"],"state":"cancelled",
            "reason":"No longer needed","idempotencyKey":"move-away"}),
    );
    assert_eq!(cancelled["task"]["state"], "cancelled");
    let restored = client.call(
        "task.move",
        json!({"workspaceId":workspace,"taskId":task["taskId"],"state":"todo",
            "reason":"Needed again","idempotencyKey":"move-back"}),
    );
    assert_eq!(restored["task"]["state"], "todo");
    let history = Store::open(&rig.path)
        .unwrap()
        .task_moves(&workspace, task["taskId"].as_str().unwrap())
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].reason, "No longer needed");
    assert_eq!(history[1].reason, "Needed again");
}

#[test]
fn refused_move_enters_attention_and_a_valid_move_clears_it() {
    let mut lifecycle = TaskLifecycleRig::new();
    let task = lifecycle.create_and_reserve("Move needs attention", "reserve-attention-move");
    let refused = lifecycle.human.request(
        "task.move",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":task.task_id,"state":"in_progress",
            "reason":"Cannot reserve by dragging","idempotencyKey":"refused-attention-move"
        }),
    );
    assert_eq!(refused["error"]["data"]["kind"], "transition_refused");
    let attention = lifecycle.human.section("attention");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["paneId"], lifecycle.pane);
    assert!(
        attention[0]["message"]
            .as_str()
            .unwrap()
            .contains("Move refused")
    );

    let moved = lifecycle.human.call(
        "task.move",
        json!({
            "workspaceId":lifecycle.workspace,"taskId":task.task_id,"state":"in_review",
            "reason":"Ready for review","idempotencyKey":"accepted-attention-move"
        }),
    );
    assert_eq!(moved["task"]["state"], "in_review");
    assert!(lifecycle.human.section("attention").is_empty());
}

#[cfg(unix)]
#[test]
fn supervisor_overflow_during_engine_outage_produces_an_explicit_replay_gap() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let (before, _) = client.tail(&pane);
    client.call("pane.input",json!({"paneId":pane,"bytes":STANDARD.encode(b"sleep 0.2; head -c 9437184 /dev/zero; printf done > overflow.done\n"),"idempotencyKey":"overflow-input"}));
    drop(client);
    rig.kill_engine();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while fs::read(rig.path.join("overflow.done")).ok().as_deref() != Some(b"done") {
        assert!(
            Instant::now() < deadline,
            "supervisor blocked terminal output"
        );
        thread::sleep(Duration::from_millis(10));
    }
    rig.start();
    let mut client = rig.client();
    // Wait until the retained output has been durably ingested by the engine.
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let (snapshot, _) = client.tail(&pane);
        if snapshot["nextOffset"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 9437184
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    client.messages.clear();
    let after = client.call(
        "pane.stream.subscribe",
        json!({"paneId":pane,"from":{"streamId":before["streamId"],"offset":before["nextOffset"]}}),
    );
    assert_eq!(after["streamId"], before["streamId"]);
    let mut saw_gap = false;
    let mut next = before["nextOffset"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    loop {
        let frame = Frame::read_from(&mut client.socket)
            .unwrap()
            .expect("delta replay");
        match frame {
            Frame::Terminal { offset, bytes, .. } => {
                assert_eq!(offset, next);
                next += bytes.len() as u64;
            }
            Frame::Json(bytes) => {
                let event: Value = serde_json::from_slice(&bytes).unwrap();
                match event["method"].as_str() {
                    Some("stream.gap") => {
                        assert_eq!(
                            event["params"]["fromOffset"]
                                .as_str()
                                .unwrap()
                                .parse::<u64>()
                                .unwrap(),
                            next
                        );
                        next = event["params"]["toOffset"]
                            .as_str()
                            .unwrap()
                            .parse()
                            .unwrap();
                        saw_gap = true;
                    }
                    Some("stream.replay_done") => {
                        assert_eq!(
                            next,
                            event["params"]["nextOffset"]
                                .as_str()
                                .unwrap()
                                .parse::<u64>()
                                .unwrap()
                        );
                        break;
                    }
                    _ => (),
                }
            }
            _ => panic!("unexpected attachment"),
        }
    }
    assert!(
        saw_gap,
        "missing bytes must not be presented as a continuous stream"
    );
}
