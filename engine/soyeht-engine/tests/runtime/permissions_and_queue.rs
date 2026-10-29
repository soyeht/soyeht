use super::*;

fn wait_for_path(path: &std::path::Path, message: &str) {
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    while !path.exists() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn terminal_spawn_refusal_finishes_the_native_request_without_restarting_the_agent() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    let prompt = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"terminal-missing","idempotencyKey":"missing"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let ended = rig
            .database_probe()
            .event_count_by_field(
                "pane.turn_ended",
                "promptId",
                prompt["promptId"].as_str().unwrap(),
            )
            .unwrap();
        if ended == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native spawn error left the agent waiting"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client.pane(&pane)["state"], "idle");
    assert_eq!(client.pane(&pane)["pid"], before["pid"]);
    assert_eq!(
        client.pane(&pane)["agent"]["sessionId"],
        before["agent"]["sessionId"]
    );
    let response = rig
        .database_probe()
        .request_result("terminal/create", &pane)
        .unwrap()
        .unwrap();
    assert_eq!(response["native"]["error"]["code"], -32603);
    let pending = rig.database_probe().open_request_count(&pane).unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn a_lost_output_page_preserves_pending_permission_and_declares_the_gap() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-overflow","idempotencyKey":"overflow"}),
    );
    client.wait_state(&pane, "working");
    rig.kill_engine();
    wait_until("fake did not fill output retention", || {
        rig.path.join("native/overflow.done").exists()
    });
    let generation = rig.database_probe().engine_generation().unwrap();
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation).unwrap();
    let Response::Instances(instances) = supervisor.request(Request::List).unwrap() else {
        panic!("process registry");
    };
    let instance = instances
        .iter()
        .find(|i| json!(i.pid) == before["pid"])
        .unwrap();
    let Response::PendingRequests {
        items,
        recovery_gap,
    } = supervisor
        .request(Request::PendingRequests {
            instance_id: instance.instance_id.clone(),
        })
        .unwrap()
    else {
        panic!("pending registry");
    };
    assert!(!recovery_gap);
    let pending = items
        .iter()
        .find(|r| r.method == "session/request_permission")
        .unwrap();
    let Response::Output(output) = supervisor
        .request(Request::Drain {
            instance_id: instance.instance_id.clone(),
            from_seq: pending.from_seq,
        })
        .unwrap()
    else {
        panic!("retained output");
    };
    assert!(
        output
            .gap
            .as_ref()
            .is_some_and(|gap| gap.to_seq >= pending.to_seq),
        "permission was not actually lost from byte retention"
    );
    drop(supervisor);
    rig.start();
    let mut client = rig.client();
    client.wait_state(&pane, "waiting_for_you");
    let requests = client.section("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(client.pane(&pane)["pid"], before["pid"]);
    let history = client.call("pane.transcript", json!({"paneId":pane}));
    assert!(history["items"].as_array().unwrap().iter().any(|m| {
        m["role"] == "system"
            && m["text"]
                .as_str()
                .is_some_and(|text| text.contains("missing section"))
    }));
    let request = requests[0]["request"]["requestId"].clone();
    client.call("pane.permission.answer", json!({"paneId":pane,"requestId":request,"optionId":"once","idempotencyKey":"answer-overflow"}));
    client.wait_state(&pane, "idle");
    assert!(client.section("requests").is_empty());
    let count = rig
        .database_probe()
        .event_count_by_field(
            "pane.permission_requested",
            "requestId",
            request.as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn an_active_prompt_leaves_the_published_queue_when_it_starts() {
    let rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let prompt = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission","idempotencyKey":"leaves-queue"}),
    );
    client.wait_state(&pane, "waiting_for_you");
    // The enqueue announced one queued prompt. Clients refresh their queue
    // row only from pane.queued, so starting the prompt must announce the
    // shrink too, not wait for the turn to end.
    let queued = rig
        .database_probe()
        .latest_event("pane.queued")
        .unwrap()
        .expect("queue event");
    assert_eq!(queued["paneId"], pane);
    assert_eq!(queued["n"], 0);
    assert_eq!(queued["promptIds"], json!([]));
    assert_eq!(
        client.section("requests")[0]["request"]["promptId"],
        prompt["promptId"]
    );
    let listed = client.call("pane.queue.list", json!({"paneId":pane,"limit":64}));
    assert!(listed["items"].as_array().unwrap().is_empty());
}

#[test]
fn agent_panes_keep_creation_order_while_one_asks_and_the_other_is_messaged() {
    let rig = Rig::new();
    let (mut client, first) = rig.agent();
    let workspace = client.pane(&first)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, second) = rig.agent_in_workspace(&workspace);
    client.call(
        "pane.prompt",
        json!({"paneId":first,"text":"permission","idempotencyKey":"order-asks"}),
    );
    client.wait_state(&first, "waiting_for_you");
    let store = Store::open(&rig.path).unwrap();
    let runtime: Value = soyeht_store::agents::runtime(&store, &first)
        .unwrap()
        .unwrap();
    let (socket, _) = binding_hello(
        &rig.path.join("engine.sock"),
        runtime["binding"].as_str().unwrap(),
    );
    let mut agent = Client {
        socket,
        next: 1,
        messages: vec![],
    };
    agent.call(
        "agent.message",
        json!({"targetPaneId":second,"text":"hello","idempotencyKey":"order-message"}),
    );
    // A reconnecting client lays out an automatic workspace in snapshot pane
    // order, so that order must be creation order, not identifier order.
    let order: Vec<_> = rig
        .client()
        .section("panes")
        .into_iter()
        .filter(|pane| pane["workspaceId"] == workspace)
        .map(|pane| pane["paneId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(order, [first, second]);
}

#[test]
fn a_permission_created_during_engine_outage_is_recovered_once() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent();
    let before = client.pane(&pane);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission-gated","idempotencyKey":"during-outage"}),
    );
    wait_for_path(
        &rig.path.join("native/permission.waiting"),
        "fake did not receive the permission prompt",
    );
    rig.kill_engine();
    let persisted = rig
        .database_probe()
        .request_count("session/request_permission")
        .unwrap();
    assert_eq!(persisted, 0, "request predates engine outage");
    fs::write(rig.path.join("native/permission.release"), b"continue").unwrap();
    let generation = rig.database_probe().engine_generation().unwrap();
    let mut supervisor = PtyClient::connect(&rig.path.join("ptyd.sock"), generation).unwrap();
    let Response::Instances(instances) = supervisor.request(Request::List).unwrap() else {
        panic!("process registry");
    };
    let instance = instances
        .iter()
        .find(|i| json!(i.pid) == before["pid"])
        .unwrap();
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let Response::PendingRequests {
            items,
            recovery_gap,
        } = supervisor
            .request(Request::PendingRequests {
                instance_id: instance.instance_id.clone(),
            })
            .unwrap()
        else {
            panic!("pending requests");
        };
        assert!(!recovery_gap);
        if items
            .iter()
            .any(|r| r.method == "session/request_permission")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fake did not ask while engine was absent"
        );
        thread::sleep(Duration::from_millis(20));
    }
    drop(supervisor);
    rig.start();
    let mut first = rig.client();
    let mut second = rig.client();
    first.wait_state(&pane, "waiting_for_you");
    let waiting = first.pane(&pane);
    assert!(waiting["agent"]["silentSince"].is_null());
    let requests = first.section("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests, second.section("requests"));
    assert_eq!(first.pane(&pane)["pid"], before["pid"]);
    assert_eq!(
        first.pane(&pane)["agent"]["sessionId"],
        before["agent"]["sessionId"]
    );
    let id = requests[0]["request"]["requestId"].clone();
    let result = first.call(
        "pane.permission.answer",
        json!({"paneId":pane,"requestId":id,"optionId":"once","idempotencyKey":"first"}),
    );
    let late = second.call(
        "pane.permission.answer",
        json!({"paneId":pane,"requestId":id,"optionId":"deny","idempotencyKey":"second"}),
    );
    assert_eq!(result, late);
    first.wait_state(&pane, "idle");
    let answered = rig
        .database_probe()
        .event_count_by_field(
            "pane.permission_answered",
            "requestId",
            id.as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(answered, 1);
}

#[test]
fn automatic_permission_is_never_presented_and_expiry_survives_restart() {
    let mut rig = Rig::new();
    let (mut client, pane) = rig.agent_announcing("none");
    let applied = client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"allow_all","idempotencyKey":"on"}),
    );
    assert_eq!(applied["effective"], "allow_all");
    let prompt = client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission","idempotencyKey":"auto"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        assert!(
            client.section("requests").is_empty(),
            "automatic permission became visible"
        );
        let done = rig
            .database_probe()
            .event_count_by_field(
                "pane.turn_ended",
                "promptId",
                prompt["promptId"].as_str().unwrap(),
            )
            .unwrap();
        if done == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "automatic permission did not finish"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let requested = rig
        .database_probe()
        .event_count("pane.permission_requested")
        .unwrap();
    assert_eq!(
        requested, 0,
        "automatic permission was published to connected clients"
    );
    let by_rule = rig
        .database_probe()
        .event_count_by_field("pane.permission_answered", "by", "rule")
        .unwrap();
    assert_eq!(by_rule, 1);
    client.call(
        "pane.config.set",
        json!({"paneId":pane,"configId":"mode","value":"ask","idempotencyKey":"off"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"permission","idempotencyKey":"expires"}),
    );
    client.wait_state(&pane, "waiting_for_you");
    let question = client.section("requests")[0]["request"].clone();
    rig.kill_engine();
    // Preserve the request and process; advance only this test deadline.
    rig.database_probe()
        .expire_request(question["requestId"].as_str().unwrap())
        .unwrap();
    rig.start();
    let mut client = rig.client();
    client.wait_state(&pane, "idle");
    assert!(client.section("requests").is_empty());
    let attention = client.section("attention");
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["attentionId"], question["requestId"]);
    assert!(
        attention[0]["message"]
            .as_str()
            .unwrap()
            .contains("expired")
    );
    assert!(attention[0]["requestId"].is_null());
    let late = client.call("pane.permission.answer", json!({"paneId":pane,"requestId":question["requestId"],"optionId":"once","idempotencyKey":"late"}));
    assert_eq!(late["result"], "expired");
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.section("attention"), attention);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"hello","idempotencyKey":"resume"}),
    );
    assert!(client.section("attention").is_empty());
}
