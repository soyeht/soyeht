use super::*;
use soyeht_protocol::ErrorKind;
pub(crate) fn state() -> State {
    State::new(
        HarnessLaunch {
            id: "test".into(),
            name: "Test".into(),
            command: "/test".into(),
            args: vec![],
            version: "1".into(),
            environment: None,
        },
        "digest".into(),
        Some("default".into()),
    )
    .unwrap()
}

#[test]
fn full_queue_fits_the_snapshot_and_pages_preserve_every_prompt_byte() {
    let mut state = state();
    for _ in 0..64 {
        state
            .enqueue(
                "\0".repeat(limits::PROMPT_BYTES),
                PromptOrigin::Human,
                "now".into(),
            )
            .unwrap();
    }
    assert!(
        state
            .enqueue("overflow".into(), PromptOrigin::Human, "now".into())
            .is_err()
    );
    assert!(serde_json::to_vec(&state.public(0)).unwrap().len() < 65536);
    let mut cursor = None;
    let mut count = 0;
    loop {
        let page = state.queue_page(cursor.as_deref(), 64, 0).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= limits::TRANSCRIPT_PAGE_BYTES);
        for item in page.items {
            assert_eq!(item.text, "\0".repeat(limits::PROMPT_BYTES));
            count += 1;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(count, 64);
    let cursor = state.queue_page(None, 1, 0).unwrap().next_cursor.unwrap();
    let first = state.queue[0].prompt_id.clone();
    state.remove(&first).unwrap();
    assert_eq!(
        errors::kind(&state.queue_page(Some(&cursor), 1, 0).unwrap_err()),
        ErrorKind::ResyncRequired
    );
    assert!(
        state
            .enqueue("🌍".repeat(16385), PromptOrigin::Human, "now".into())
            .is_err()
    );
}

#[test]
fn public_state_restores_an_in_progress_harness_switch() {
    let mut state = state();
    state.harness_switch = Some(HarnessSwitchCheckpoint {
        user: crate::agents::pending::UserOperation {
            scope: "person".into(),
            key: "switch-once".into(),
        },
        from_harness: "first".into(),
        to_harness: "second".into(),
        to_name: "Second".into(),
        candidate_instance_key: "candidate".into(),
        previous_queue_paused: false,
    });

    let public = state.public(0);
    let switch = public.harness_switch.unwrap();
    assert_eq!(switch.from_harness, "first");
    assert_eq!(switch.to_harness, "second");
}

#[test]
fn checkpoints_from_before_native_modes_do_not_hold_the_queue() {
    for phase in ["NeedsLogin", "Faulted"] {
        let mut legacy = serde_json::to_value(state()).unwrap();
        legacy["phase"] = serde_json::json!(phase);
        legacy["requested_options"] = serde_json::json!({"permission": "ask"});
        legacy["initial_config_failed"] = serde_json::json!(true);
        legacy["permission"] = serde_json::json!("ask");
        legacy["yolo"] = serde_json::json!(false);

        let mut restored: State = serde_json::from_value(legacy).unwrap();

        assert!(restored.requested_options.is_empty(), "{phase}");
        restored.phase = Phase::Ready;
        let prompt = restored
            .enqueue("after upgrade".into(), PromptOrigin::Human, "now".into())
            .unwrap();
        let taken = restored.take_next(1, 0, false).unwrap().unwrap();
        assert_eq!(taken.prompt_id, prompt.prompt_id, "{phase}");
        assert!(restored.queue.is_empty(), "{phase}");
    }
}

#[test]
fn checkpoints_from_before_reviewer_isolation_keep_the_task_server_enabled() {
    let serialized = serde_json::to_value(state()).unwrap();
    let mut object = serialized.as_object().unwrap().clone();
    object.remove("mcp_enabled");

    let restored: State = serde_json::from_value(serde_json::Value::Object(object)).unwrap();

    assert!(restored.mcp_enabled);
}

#[test]
fn task_restart_keeps_binding_and_transcript_sequence_but_opens_a_fresh_session() {
    let mut prior = state();
    prior.session_id = Some("old-session".into());
    prior.next_message_seq = 17;
    let preset = TaskExecutor {
        workspace_default: false,
        harness: Some("task-harness".into()),
        model: Some("task-model".into()),
        effort: Some("high".into()),
        permission_mode: Some("acceptEdits".into()),
    };
    let launch = HarnessLaunch {
        id: "task-harness".into(),
        name: "Task harness".into(),
        command: "/task".into(),
        args: vec![],
        version: "2".into(),
        environment: None,
    };

    let restarted = prior
        .restart_for_task(launch, "task-digest".into(), &preset)
        .unwrap();

    assert_eq!(restarted.binding, prior.binding);
    assert_ne!(restarted.hook_binding, prior.hook_binding);
    assert_eq!(restarted.next_message_seq, 17);
    assert!(restarted.session_id.is_none());
    assert!(restarted.queue.is_empty());
    assert_eq!(restarted.mode.as_deref(), Some("acceptEdits"));
    assert_eq!(
        restarted.requested_options.get("model"),
        Some(&ConfigValue::V0("task-model".into()))
    );
}

#[test]
fn restored_queue_obeys_draft_priority_open_requests_and_serial_changes() {
    let mut state = state();
    state.phase = Phase::Ready;
    state.draft_until_ms = 1000;
    let automatic = state
        .enqueue("automatic".into(), PromptOrigin::Automatic, "now".into())
        .unwrap();
    assert!(state.take_next(1, 1, false).unwrap().is_none());
    let human = state
        .enqueue("human".into(), PromptOrigin::Human, "now".into())
        .unwrap();
    assert!(state.take_next(1, 1, false).unwrap().is_none());
    state.requested_options.clear(); // Native initial configuration completed.
    assert!(state.take_next(1, 1, true).unwrap().is_none());
    assert_eq!(
        state.take_next(1, 1, false).unwrap().unwrap().prompt_id,
        human.prompt_id
    );
    let mut restored: State = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    restored.queue = state.queue.clone(); // Hydrated separately from ordered queue rows.
    assert!(restored.take_next(1, 2000, false).unwrap().is_none());
    restored.active = None;
    assert_eq!(
        restored
            .take_next(2, 2000, false)
            .unwrap()
            .unwrap()
            .prompt_id,
        automatic.prompt_id
    );
    assert_eq!(restored.active.unwrap().generation, Some(2));
}

#[test]
fn feedback_is_queued_after_people_and_before_loop_ticks() {
    let mut state = state();
    state
        .enqueue_identified(
            "loop:one".into(),
            "pull work".into(),
            PromptOrigin::Automatic,
            "now".into(),
        )
        .unwrap();
    let person = state
        .enqueue("person".into(), PromptOrigin::Human, "now".into())
        .unwrap();
    state
        .enqueue_feedback("task-feedback:WS-1:1".into(), "fix CI".into(), "now".into())
        .unwrap();
    assert_eq!(
        state
            .queue
            .iter()
            .map(|item| item.prompt_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            person.prompt_id.as_str(),
            "task-feedback:WS-1:1",
            "loop:one"
        ]
    );

    state.phase = Phase::Ready;
    state.requested_options.clear();
    let mut delivered = Vec::new();
    for generation in 1..=3 {
        let prompt = state.take_next(generation, 1, false).unwrap().unwrap();
        delivered.push(prompt.text);
        state.active = None;
    }
    assert_eq!(delivered, vec!["person", "fix CI", "pull work"]);
}

#[test]
fn restored_queue_recovers_source_priority_without_changing_fifo_order() {
    let mut state = state();
    for (id, text, origin) in [
        ("loop:one", "loop one", PromptOrigin::Automatic),
        ("task-feedback:one", "feedback one", PromptOrigin::Automatic),
        ("person-one", "person one", PromptOrigin::Human),
        ("loop:two", "loop two", PromptOrigin::Automatic),
        ("task-feedback:two", "feedback two", PromptOrigin::Automatic),
        ("person-two", "person two", PromptOrigin::Human),
    ] {
        state.queue.push_back(PromptItem {
            prompt_id: id.into(),
            text: text.into(),
            origin,
            status: PromptStatus::Queued,
            queued_at: "now".into(),
            generation: None,
            attachments: None,
            agent_message: None,
        });
    }
    let restored = state.queue.drain(..).collect();
    state.restore_queue(restored);
    assert_eq!(
        state
            .queue
            .iter()
            .map(|item| item.prompt_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "person-one",
            "person-two",
            "task-feedback:one",
            "task-feedback:two",
            "loop:one",
            "loop:two"
        ]
    );
}
