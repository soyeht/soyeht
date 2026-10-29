use super::*;

#[test]
fn snapshot_cursor_replays_changes_exactly_once_before_live_events() {
    let rig = Rig::new();
    let mut observer = rig.client();
    let mut writer = rig.client();
    let (workspace, _) = writer.create(&rig);
    let (snapshot, _) = observer.snapshot();
    for n in 0..20 {
        writer.call("workspace.rename",json!({"workspaceId":workspace,"name":format!("Name {n}"),"idempotencyKey":Uuid::new_v4().to_string()}));
    }
    observer.messages.clear();
    observer.call("events.subscribe", json!({"since":snapshot["cursor"]}));
    let events: Vec<_> = observer
        .messages
        .iter()
        .filter_map(|f| {
            if let Frame::Json(bytes) = f {
                let v: Value = serde_json::from_slice(bytes).unwrap();
                (v["method"] == "workspace.renamed").then_some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(events.len(), 20);
    let mut prior = snapshot["cursor"].as_str().unwrap().parse::<u64>().unwrap();
    for (n, event) in events.iter().enumerate() {
        let id = event["params"]["eventId"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(id > prior);
        prior = id;
        assert_eq!(event["params"]["workspace"]["name"], format!("Name {n}"));
    }
    // Engine-wide notifications (`servers.local.changed`, `skills.changed`) reach every
    // subscriber whenever their scan finishes, so the next frame is not necessarily the
    // rename. The next rename must still be the live one: a replayed duplicate would be first.
    observer.messages.clear();
    writer.call(
        "workspace.rename",
        json!({"workspaceId":workspace,"name":"Live","idempotencyKey":Uuid::new_v4().to_string()}),
    );
    let event = observer.wait_for_notification("workspace.renamed", |_| true);
    assert_eq!(event["workspace"]["name"], "Live");
    assert!(event["eventId"].as_str().unwrap().parse::<u64>().unwrap() > prior);
}

#[test]
fn backpressured_subscriber_survives_an_answer_from_a_second_subscriber() {
    let rig = Rig::new();
    let (mut answerer, pane) = rig.agent();
    let workspace = answerer.pane(&pane)["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut observer = rig.client();
    let (observer_snapshot, _) = observer.snapshot();
    observer.call(
        "events.subscribe",
        json!({"since":observer_snapshot["cursor"]}),
    );
    let (answerer_snapshot, _) = answerer.snapshot();
    answerer.call(
        "events.subscribe",
        json!({"since":answerer_snapshot["cursor"]}),
    );

    answerer.call(
        "workspace.rename",
        json!({
            "workspaceId":workspace,
            "name":"x".repeat(256 * 1024),
            "idempotencyKey":"backpressure-event",
        }),
    );
    answerer.call(
        "pane.prompt",
        json!({
            "paneId":pane,
            "text":"permission",
            "idempotencyKey":"backpressure-prompt",
        }),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let request = loop {
        if let Some(entry) = answerer
            .section("requests")
            .into_iter()
            .find(|entry| entry["kind"] == "permission")
        {
            break entry["request"]["requestId"].clone();
        }
        assert!(Instant::now() < deadline, "permission request did not open");
        thread::sleep(Duration::from_millis(20));
    };
    answerer.call(
        "pane.permission.answer",
        json!({
            "paneId":pane,
            "requestId":request,
            "optionId":"once",
            "idempotencyKey":"backpressure-answer",
        }),
    );
    answerer.wait_state(&pane, "idle");

    // This exceeds the former per-write deadline while remaining far below
    // the contractual 8 MiB connection budget.
    thread::sleep(BACKPRESSURE_OBSERVATION_WINDOW);
    observer.call("engine.status", json!({}));
    assert!(observer.messages.iter().any(|frame| {
        let Frame::Json(bytes) = frame else {
            return false;
        };
        serde_json::from_slice::<Value>(bytes)
            .is_ok_and(|value| value["method"] == "pane.permission_answered")
    }));
}

#[cfg(unix)]
#[test]
fn reconnect_replays_screen_and_duplicate_input_is_not_typed_twice() {
    let rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let input = json!({"paneId":pane,"bytes":STANDARD.encode(b"printf x >> once; printf 'UNIQUE_OUTPUT_42\\n'\n"),"idempotencyKey":"input-1"});
    let receipt = client.call("pane.input", input.clone());
    assert_eq!(client.call("pane.input", input), receipt);
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let (info, frames) = loop {
        let replay = client.tail(&pane);
        let bytes: Vec<_> = replay
            .1
            .iter()
            .filter_map(|f| {
                if let Frame::Terminal { bytes, .. } = f {
                    Some(bytes.as_slice())
                } else {
                    None
                }
            })
            .flatten()
            .copied()
            .collect();
        if bytes
            .windows(16)
            .filter(|s| *s == b"UNIQUE_OUTPUT_42")
            .count()
            >= 2
            // A shell redraw can echo the marker twice before executing it.
            // Wait for the actual effect as well as its terminal output.
            && fs::read(rig.path.join("once")).ok().as_deref() == Some(b"x")
        {
            break replay;
        }
        assert!(
            Instant::now() < deadline,
            "output not recorded: {}",
            String::from_utf8_lossy(&bytes)
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(fs::read(rig.path.join("once")).unwrap(), b"x");
    let mut expected = 0;
    for frame in &frames {
        if let Frame::Json(bytes) = frame {
            let v: Value = serde_json::from_slice(bytes).unwrap();
            if v["method"] == "stream.replay_start" {
                expected = v["params"]["fromOffset"].as_str().unwrap().parse().unwrap();
            }
        }
        if let Frame::Terminal { offset, bytes, .. } = frame {
            assert_eq!(*offset, expected);
            expected += bytes.len() as u64;
        }
    }
    assert_eq!(
        expected,
        info["nextOffset"].as_str().unwrap().parse::<u64>().unwrap()
    );
    let pid = client.pane(&pane)["pid"].clone();
    drop(client);
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["pid"], pid);
    let (_, replayed) = client.tail(&pane);
    assert!(!replayed.is_empty());
}

#[test]
fn persisted_terminal_capture_records_provenance_and_resumes_sequence_after_restart() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    drop(client);
    rig.kill_engine();
    rig.database_probe()
        .set_experiment_capture(&pane, true)
        .unwrap();
    rig.start();

    let root = rig.path.join("experiments/pane-state");
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["experimentCapture"]["on"], true);
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(b"printf 'CAPTURE_ONE\\n'\n"),
            "idempotencyKey":"capture-one"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let first_rows = loop {
        let rows = experiment_rows(&root);
        if rows
            .iter()
            .any(|row| row["kind"] == "observation" && row.to_string().contains("CAPTURE_ONE"))
        {
            break rows;
        }
        assert!(
            Instant::now() < deadline,
            "captured output was not recorded"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let metadata = first_rows
        .iter()
        .find(|row| row["kind"] == "metadata")
        .unwrap();
    let binary = fs::read(env!("CARGO_BIN_EXE_soyeht-engine-test")).unwrap();
    assert_eq!(
        metadata["engine"]["executableSha256"],
        format!("{:x}", Sha256::digest(binary))
    );
    let first_sequence = first_rows
        .iter()
        .filter(|row| row["kind"] == "observation")
        .filter_map(|row| row["sequence"].as_u64())
        .max()
        .unwrap();

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["experimentCapture"]["on"], true);
    client.call(
        "pane.input",
        json!({"paneId":pane,"bytes":STANDARD.encode(b"printf 'CAPTURE_TWO\\n'\n"),
            "idempotencyKey":"capture-two"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    loop {
        let rows = experiment_rows(&root);
        if rows.iter().any(|row| {
            row["kind"] == "observation"
                && row["sequence"]
                    .as_u64()
                    .is_some_and(|value| value > first_sequence)
                && row.to_string().contains("CAPTURE_TWO")
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "capture did not resume after restart"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn persisted_agent_capture_records_real_protocol_transitions_as_labels() {
    let mut rig = Rig::new();
    let (client, pane) = rig.agent();
    drop(client);
    rig.kill_engine();
    rig.database_probe()
        .set_experiment_capture(&pane, true)
        .unwrap();
    rig.start();

    let root = rig.path.join("experiments/pane-state");
    let mut client = rig.client();
    wait_until("agent session did not recover", || {
        client.pane(&pane)["agent"]["sessionId"].is_string()
    });
    assert_eq!(client.pane(&pane)["experimentCapture"]["on"], true);
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"capture-protocol-labels",
            "idempotencyKey":"capture-protocol-labels"}),
    );

    let labels = eventually("ACP transitions did not reach experiment labels", || {
        let labels = experiment_rows(&root)
            .into_iter()
            .filter(|row| row["kind"] == "label" && row["pane_id"] == pane)
            .collect::<Vec<_>>();
        let states = labels
            .iter()
            .filter_map(|row| row["state"].as_str())
            .collect::<Vec<_>>();
        if states.contains(&"working") && states.contains(&"idle") {
            Ok(labels.clone())
        } else {
            Err(format!("{states:?}"))
        }
    });
    assert!(labels.iter().all(|row| row["pane_kind"] == "agent"));
    assert!(labels.iter().all(|row| row["source"] == "protocol"));
    assert!(labels.iter().all(|row| {
        row["harness"]["identity"] == "known" && row["harness"]["id"] == "test-agent"
    }));
    assert!(labels.windows(2).all(|rows| {
        rows[0]["sequence"].as_u64().unwrap() < rows[1]["sequence"].as_u64().unwrap()
    }));

    rig.install_switch_harnesses();
    client.call(
        "pane.harness.switch",
        json!({"paneId":pane,"harness":"next-agent",
            "idempotencyKey":"captured-harness-switch"}),
    );
    client.call(
        "pane.prompt",
        json!({"paneId":pane,"text":"capture-next-harness",
            "idempotencyKey":"capture-next-harness"}),
    );
    let deadline = Instant::now() + ASYNC_ASSERTION_TIMEOUT;
    let rows = loop {
        let rows = experiment_rows(&root);
        let new_states = rows
            .iter()
            .filter(|row| row["kind"] == "label" && row["harness"]["id"] == "next-agent")
            .filter_map(|row| row["state"].as_str())
            .collect::<Vec<_>>();
        if new_states.contains(&"working") && new_states.contains(&"idle") {
            break rows;
        }
        assert!(
            Instant::now() < deadline,
            "switched harness labels did not reach their own segment: {new_states:?}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        rows.iter()
            .any(|row| { row["kind"] == "segment_end" && row["harness"]["id"] == "test-agent" })
    );
    assert!(
        rows.iter()
            .any(|row| { row["kind"] == "metadata" && row["harness"]["id"] == "next-agent" })
    );
    let experiment_text = serde_json::to_string(&rows).unwrap();
    assert!(!experiment_text.contains("capture-protocol-labels"));
    assert!(!experiment_text.contains("capture-next-harness"));
    assert!(!rows.iter().any(|row| row["kind"] == "observation"));
}
