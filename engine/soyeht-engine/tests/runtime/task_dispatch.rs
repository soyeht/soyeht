use super::*;

fn create_task_fixture(rig: &Rig, client: &mut Client) -> (String, Value, Value) {
    let workspace = create_workspace(client, "Tasks", &rig.path, "task-workspace");
    let invalid = client.request(
        "task.create",
        json!({"workspaceId":workspace,"title":"Invalid","mustNotChange":"","idempotencyKey":"invalid-task"}),
    );
    assert_eq!(invalid["error"]["data"]["kind"], "invalid_params");
    let first_params = json!({
        "workspaceId":workspace,"title":"First","description":"Build it",
        "mustNotChange":"Keep the boundary","idempotencyKey":"first-task"
    });
    let first = client.call("task.create", first_params.clone())["task"].clone();
    let repeated = client.call("task.create", first_params)["task"].clone();
    assert_eq!(repeated["taskId"], first["taskId"]);
    assert_eq!(first["taskId"], "WS-1");
    assert_eq!(first["createdBy"], "you");
    let second = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Second","mustNotChange":"Keep the first task",
            "priority":"urgent","dependencies":[first["taskId"]],"idempotencyKey":"second-task"
        }),
    )["task"]
        .clone();
    let page = client.call("task.list", json!({"workspaceId":workspace,"limit":1}));
    assert_eq!(page["items"][0]["taskId"], second["taskId"]);
    assert_eq!(page["nextCursor"], "1");
    let next = client.call(
        "task.list",
        json!({"workspaceId":workspace,"limit":1,"cursor":page["nextCursor"]}),
    );
    assert_eq!(next["items"][0]["taskId"], first["taskId"]);
    assert!(next["nextCursor"].is_null());
    (workspace, first, second)
}

#[cfg(unix)]
#[test]
fn create_and_dispatch_resumes_after_an_effect_failure_without_duplicates() {
    let rig = Rig::new();
    rig.install_fake_harness("config");
    let mut client = rig.client();
    let workspace = create_workspace(
        &mut client,
        "Create dispatch",
        &rig.path,
        "create-dispatch-workspace",
    );
    let worktrunk = rig.path.join("bin/wt");
    let healthy_worktrunk = replace_link_with_script(&worktrunk, "#!/bin/sh\nexit 2\n");
    let params = json!({
        "workspaceId":workspace,"title":"Create and dispatch","description":"Run both steps",
        "howToVerify":"Observe one pane","mustNotChange":"Do not duplicate effects",
        "executor":{"workspaceDefault":false,"harness":"test-agent","permission":"ask"},
        "createWorktree":true,"dispatch":true,"idempotencyKey":"create-dispatch-once"
    });
    let failed = client.request("task.create", params.clone());
    assert!(failed["error"].is_object(), "{failed}");

    restore_link(&worktrunk, &healthy_worktrunk);
    let resumed = client.call("task.create", params.clone());
    assert_eq!(client.call("task.create", params), resumed);
    let pane = resumed["paneId"].as_str().unwrap();
    let task = &resumed["task"];
    assert_eq!(task["taskId"], "WS-1");
    assert_eq!(task["state"], "in_progress");
    assert_eq!(task["ownerPaneId"], pane);

    let shown = eventually("dispatch did not resume", || {
        let shown = client.call(
            "task.show",
            json!({"workspaceId":workspace,"taskId":task["taskId"]}),
        );
        if shown["task"]["worktree"]["path"].is_string()
            && client.pane(pane)["agent"]["sessionId"].is_string()
        {
            Ok(shown)
        } else {
            Err(shown.to_string())
        }
    });
    assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
    let tasks = client.call("task.list", json!({"workspaceId":workspace,"limit":100}));
    assert_eq!(tasks["items"].as_array().unwrap().len(), 1);
    let worktrees = fs::read_dir(rig.path.join(".test-worktrees"))
        .unwrap()
        .count();
    assert_eq!(worktrees, 1);
}

#[cfg(unix)]
#[test]
fn worktree_cleanup_refuses_process_dirty_and_unreachable_then_keeps_the_branch() {
    let rig = Rig::new();
    rig.install_fake_harness("config");
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Cleanup", &rig.path, "cleanup-workspace");
    let task = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Clean safely","mustNotChange":"Keep the branch",
            "idempotencyKey":"cleanup-task"
        }),
    )["task"]
        .clone();
    let dispatched = client.call(
        "task.dispatch",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],
            "preset":{"workspaceDefault":false,"harness":"test-agent"},
            "idempotencyKey":"cleanup-dispatch"
        }),
    );
    let pane = dispatched["paneId"].as_str().unwrap();
    let shown = eventually("worktree was not prepared", || {
        let shown = client.call(
            "task.show",
            json!({"workspaceId":workspace,"taskId":task["taskId"]}),
        );
        if shown["task"]["worktree"]["path"].is_string() && client.agent_started(pane) {
            Ok(shown)
        } else {
            Err(shown.to_string())
        }
    });
    let path = PathBuf::from(shown["task"]["worktree"]["path"].as_str().unwrap());
    let clean = |client: &mut Client, key: &str| {
        client.call(
            "task.worktree.clean",
            json!({
                "workspaceId":workspace,"taskId":task["taskId"],"idempotencyKey":key
            }),
        )
    };

    let active = clean(&mut client, "cleanup-active");
    assert_eq!(active["cleaned"], false);
    assert!(active["reason"].as_str().unwrap().contains("process"));

    client.call(
        "pane.close",
        json!({"paneId":pane,"idempotencyKey":"cleanup-close-pane"}),
    );
    fs::write(path.join(".dirty"), "dirty").unwrap();
    let dirty = clean(&mut client, "cleanup-dirty");
    assert_eq!(dirty["cleaned"], false);
    assert!(dirty["reason"].as_str().unwrap().contains("uncommitted"));

    fs::remove_file(path.join(".dirty")).unwrap();
    fs::write(path.join(".unreachable"), "unreachable").unwrap();
    let unreachable = clean(&mut client, "cleanup-unreachable");
    assert_eq!(unreachable["cleaned"], false);
    assert!(
        unreachable["reason"]
            .as_str()
            .unwrap()
            .contains("not reachable")
    );

    fs::remove_file(path.join(".unreachable")).unwrap();
    let cleaned = clean(&mut client, "cleanup-success");
    assert_eq!(cleaned["cleaned"], true);
    assert!(!path.exists());
    assert_eq!(clean(&mut client, "cleanup-success"), cleaned);
}

#[test]
fn task_next_restarts_the_same_pane_in_the_claimed_worktree() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Pull", &rig.path, "pull-workspace");
    let (mut client, pane) = rig.open_agent(client, &json!(workspace));
    let before = client.pane(&pane);
    let task = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Pulled task","description":"Run in a new session",
            "mustNotChange":"Keep the pane","idempotencyKey":"pull-task"
        }),
    )["task"]
        .clone();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"task-claim","idempotencyKey":"pull-turn"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let shown = client.call(
            "task.show",
            json!({"workspaceId":workspace,"taskId":task["taskId"]}),
        );
        let current = client.pane(&pane);
        if shown["task"]["ownerPaneId"] == pane
            && shown["task"]["worktree"]["path"].is_string()
            && current["generation"].as_u64().unwrap() > before["generation"].as_u64().unwrap()
            && current["cwd"] == shown["task"]["worktree"]["path"]
            && current["agent"]["sessionId"].is_string()
            && current["agent"]["sessionId"] != before["agent"]["sessionId"]
        {
            assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
            assert_eq!(current["name"], "Pulled task");
            let transcript = client.call("pane.transcript", json!({"paneId":pane}));
            if transcript_contains(&transcript, "\"willRestartSession\":true")
                && transcript_contains(&transcript, "# Pulled task\n\nRun in a new session")
            {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "task.next did not restart the pane: task={shown} pane={current}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn two_idle_panes_claim_one_task_exactly_once() {
    let rig = Rig::new();
    let mut client = rig.client();
    let workspace = create_workspace(&mut client, "Race", &rig.path, "race-workspace");
    let (client, first_pane) = rig.open_agent(client, &json!(workspace));
    let (mut client, second_pane) = rig.open_agent(client, &json!(workspace));
    let task = client.call(
        "task.create",
        json!({
            "workspaceId":workspace,"title":"One owner","description":"Claim once",
            "mustNotChange":"Keep one attempt","idempotencyKey":"race-task"
        }),
    )["task"]
        .clone();
    for (pane, key) in [(&first_pane, "race-first"), (&second_pane, "race-second")] {
        client.call(
            "pane.prompt",
            json!({"paneId":pane,"text":"task-claim","idempotencyKey":key}),
        );
    }
    let show = json!({"workspaceId":workspace,"taskId":task["taskId"]});
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        // Transcripts before task.show: task.next commits the claim before it replies.
        let transcripts = [&first_pane, &second_pane]
            .map(|pane| client.call("pane.transcript", json!({"paneId":pane})));
        let seen = |needle| {
            transcripts
                .iter()
                .filter(|value| transcript_contains(value, needle))
                .count()
        };
        let (restarts, empties) = (seen("\"willRestartSession\":true"), seen("\"empty\":true"));
        let shown = client.call("task.show", show.clone());
        assert!(restarts <= 1, "both idle panes claimed the task: {shown}");
        if restarts == 1 && empties == 1 {
            assert_eq!(shown["attempts"].as_array().unwrap().len(), 1);
            let owner = shown["task"]["ownerPaneId"].as_str().unwrap();
            assert!(owner == first_pane || owner == second_pane);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "task.next race did not settle once: task={shown}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn loop_for(client: &mut Client, pane: &str) -> Value {
    client
        .section("loops")
        .into_iter()
        .find(|item| item["paneId"] == pane)
        .expect("loop in snapshot")
}

const CHIEF_ROLE_PROMPT: &str = "Coordinate; do not implement. Create small, self-contained tasks with how to verify. Never depend on another task's memory. Dispatch only when no free pane is available to claim.";

fn appoint_chief(rig: &Rig, human: &mut Client, pane: &str) -> String {
    human.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"ready","idempotencyKey":"prime-lead-pane"}),
    );
    human.wait_state(pane, "idle");
    human.call(
        "pane.role.set",
        json!({"paneId":pane,"role":"chief","idempotencyKey":"appoint-lead"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let session = loop {
        let current = human.pane(pane);
        let session = current["agent"]["sessionId"].as_str().unwrap_or_default();
        if current["state"] == "idle"
            && current["agent"]["role"] == "chief"
            && !session.is_empty()
            && native_prompt_count(rig, session, CHIEF_ROLE_PROMPT) == 1
        {
            break session.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "chief role was not applied: {current}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    human.call(
        "pane.role.set",
        json!({"paneId":pane,"role":"chief","idempotencyKey":"keep-lead"}),
    );
    thread::sleep(Duration::from_millis(100));
    assert_eq!(native_prompt_count(rig, &session, CHIEF_ROLE_PROMPT), 1);
    session
}

#[test]
fn pane_loop_stops_after_three_empty_task_results_and_explicit_off() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"ready","idempotencyKey":"prime-loop-pane"}),
    );
    client.wait_state(&pane, "idle");
    client.call(
        "loop.set",
        json!({
            "paneId":pane,"prompt":"task-claim","interval":1,"maxRounds":10,
            "stopWhen":"three_empty","enabled":true,"idempotencyKey":"three-empty-loop"
        }),
    );

    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let saved = loop_for(&mut client, &pane);
        if saved["config"]["enabled"] == false
            && saved["config"]["emptyResults"] == 3
            && client.pane(&pane)["state"] == "idle"
        {
            break;
        }
        assert!(Instant::now() < deadline, "loop did not stop: {saved}");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 3);
    assert_eq!(rig.database_probe().event_count("loop.stopped").unwrap(), 1);
    assert_eq!(native_prompt_count(&rig, &session, "task-claim"), 3);
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 3);

    client.call(
        "loop.set",
        json!({
            "paneId":pane,"prompt":"","interval":0,"maxRounds":0,
            "stopWhen":"explicit_off","enabled":false,"idempotencyKey":"explicit-off"
        }),
    );
    assert_eq!(rig.database_probe().event_count("loop.stopped").unwrap(), 2);
    assert_eq!(loop_for(&mut client, &pane)["config"]["enabled"], false);
}

#[test]
fn pane_loop_waits_while_busy_and_dispatches_only_one_due_round() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let session = client.pane(&pane)["agent"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"slow","idempotencyKey":"busy-turn"}),
    );
    client.wait_state(&pane, "working");
    client.call(
        "loop.set",
        json!({
            "paneId":pane,"prompt":"loop-after-idle","interval":1,"maxRounds":1,
            "stopWhen":"round_limit","enabled":true,"idempotencyKey":"busy-loop"
        }),
    );
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 0);

    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let saved = loop_for(&mut client, &pane);
        if saved["config"]["enabled"] == false
            && saved["config"]["rounds"] == 1
            && client.pane(&pane)["state"] == "idle"
            && native_prompt_count(&rig, &session, "loop-after-idle") == 1
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "due round did not finish: {saved}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rig.database_probe().event_count("loop.tick").unwrap(), 1);
    assert_eq!(native_prompt_count(&rig, &session, "loop-after-idle"), 1);
}

#[test]
fn lead_role_prompt_and_task_authority_are_applied_to_the_live_session() {
    let mut rig = Rig::new();
    let (mut human, pane) = rig.agent();
    let workspace = human.pane(&pane)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    appoint_chief(&rig, &mut human, &pane);
    rig.kill_engine();
    rig.start();
    human = rig.client();
    assert_eq!(human.pane(&pane)["agent"]["role"], "chief");

    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut lead, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "chief");
    let created = binding_request(
        &mut lead,
        2,
        "task.create",
        json!({
            "workspaceId":workspace,"title":"Lead-created task",
            "mustNotChange":"Keep the task contract","idempotencyKey":"lead-create"
        }),
    );
    let task = created["result"]["task"].clone();
    let listed = binding_request(
        &mut lead,
        3,
        "task.list",
        json!({"workspaceId":workspace,"limit":20}),
    );
    assert_eq!(listed["result"]["items"][0]["taskId"], task["taskId"]);
    let comment = binding_request(
        &mut lead,
        4,
        "task.comment",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"text":"Ready to dispatch",
            "idempotencyKey":"lead-comment"
        }),
    );
    assert_eq!(comment["result"]["comment"]["text"], "Ready to dispatch");
    let dispatched = binding_request(
        &mut lead,
        5,
        "task.dispatch",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],
            "preset":{"workspaceDefault":true},"idempotencyKey":"lead-dispatch"
        }),
    );
    let worker_pane = dispatched["result"]["paneId"].as_str().unwrap();
    let busy_role = human.request(
        "pane.role.set",
        json!({"paneId":worker_pane,"role":"chief","idempotencyKey":"busy-role"}),
    );
    assert_eq!(busy_role["error"]["data"]["kind"], "pane_busy");
    let current = human.wait_state(worker_pane, "idle");
    let worker_binding = rig.database_probe().agent_binding(worker_pane).unwrap();
    let (mut worker, worker_welcome) =
        binding_hello(&rig.path.join("engine.sock"), &worker_binding);
    assert_eq!(worker_welcome["result"]["role"], "agent");
    let blocked = binding_request(
        &mut worker,
        6,
        "task.block",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"kind":"missing_information",
            "reason":"Need a decision","paneId":worker_pane,
            "sessionGeneration":current["generation"],"attempt":dispatched["result"]["attempt"],
            "idempotencyKey":"worker-block"
        }),
    );
    assert_eq!(
        blocked["result"]["task"]["blocking"]["kind"],
        "missing_information"
    );
    let unblocked = binding_request(
        &mut lead,
        7,
        "task.unblock",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"reason":"Decision supplied",
            "idempotencyKey":"lead-unblock"
        }),
    );
    assert!(unblocked["result"]["task"]["blocking"].is_null());
    let denied = binding_request(
        &mut lead,
        8,
        "task.complete",
        json!({
            "workspaceId":workspace,"taskId":task["taskId"],"summary":"Not mine",
            "contractChanges":[],"outOfScopeEdits":[],"idempotencyKey":"lead-complete"
        }),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");
}

fn transcript_contains(transcript: &Value, needle: &str) -> bool {
    transcript["items"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(needle))
        })
    })
}

fn assert_task_mutations_and_snapshot(
    client: &mut Client,
    workspace: &str,
    first: &Value,
    second: &Value,
) {
    let update = client.call(
        "task.update",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"version":1,
            "title":"First revised","idempotencyKey":"update-first"
        }),
    );
    assert_eq!(update["task"]["version"], 2);
    let stale = client.request(
        "task.update",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"version":1,
            "title":"Stale","idempotencyKey":"stale-first"
        }),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");
    let cycle = client.request(
        "task.update",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"version":2,
            "dependencies":[second["taskId"]],"idempotencyKey":"cycle-first"
        }),
    );
    assert_eq!(cycle["error"]["data"]["kind"], "dependency_cycle");

    let subitem = client.call(
        "task.subitem.add",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"text":"Plan",
            "idempotencyKey":"subitem-add"
        }),
    )["subitem"]
        .clone();
    assert_eq!(subitem["subitemId"], "WS-1-T01");
    let done = client.call(
        "task.subitem.done",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],
            "subitemId":subitem["subitemId"],"idempotencyKey":"subitem-done"
        }),
    );
    assert_eq!(done["subitem"]["state"], "done");
    for (key, text) in [("comment-one", "Started"), ("comment-two", "Finished")] {
        client.call(
            "task.comment",
            json!({
                "workspaceId":workspace,"taskId":first["taskId"],"text":text,
                "idempotencyKey":key
            }),
        );
    }
    let comments = client.call(
        "task.comments",
        json!({"workspaceId":workspace,"taskId":first["taskId"],"limit":1}),
    );
    assert_eq!(comments["items"][0]["text"], "Finished");
    let older = client.call(
        "task.comments",
        json!({
            "workspaceId":workspace,"taskId":first["taskId"],"limit":1,
            "before":comments["nextCursor"]
        }),
    );
    assert_eq!(older["items"][0]["text"], "Started");

    let shown = client.call(
        "task.show",
        json!({"workspaceId":workspace,"taskId":first["taskId"]}),
    );
    assert_eq!(shown["task"]["title"], "First revised");
    assert_eq!(shown["subitems"].as_array().unwrap().len(), 1);
    assert_eq!(shown["comments"].as_array().unwrap().len(), 2);
    let (_, parts) = client.snapshot();
    assert_eq!(
        parts
            .iter()
            .filter(|part| part["section"] == "tasks")
            .count(),
        1
    );
    assert_eq!(
        parts
            .iter()
            .find(|part| part["section"] == "tasks")
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

fn assert_executor_cannot_read_unowned_tasks(rig: &Rig, workspace: &str, first: &Value) {
    let (_, pane) = rig.agent();
    let binding = rig.database_probe().agent_binding(&pane).unwrap();
    let (mut bound, welcome) = binding_hello(&rig.path.join("engine.sock"), &binding);
    assert_eq!(welcome["result"]["role"], "agent");
    let denied = binding_request(
        &mut bound,
        2,
        "task.show",
        json!({"workspaceId":workspace,"taskId":first["taskId"]}),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");
    let denied = binding_request(
        &mut bound,
        3,
        "task.list",
        json!({"workspaceId":workspace,"limit":20}),
    );
    assert_eq!(denied["error"]["data"]["kind"], "forbidden_role");
}

#[test]
fn task_authoring_is_durable_paged_versioned_and_closed_by_role() {
    let rig = Rig::new();
    let mut client = rig.client();
    let (workspace, first, second) = create_task_fixture(&rig, &mut client);
    assert_task_mutations_and_snapshot(&mut client, &workspace, &first, &second);
    assert_executor_cannot_read_unowned_tasks(&rig, &workspace, &first);
}
