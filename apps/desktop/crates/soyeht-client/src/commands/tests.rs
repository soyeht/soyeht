use super::*;
use serde_json::Value;
use std::sync::mpsc;

fn input(pane: &str, text: &str) -> Command {
    Command::Input {
        pane_id: pane.into(),
        bytes: text.as_bytes().to_vec(),
    }
}

#[test]
fn typing_bursts_for_one_pane_merge_in_order() {
    let (tx, rx) = mpsc::channel();
    tx.send(input("p", "b")).unwrap();
    tx.send(input("p", "c")).unwrap();
    let mut carry = None;
    assert_eq!(
        coalesce(input("p", "a"), &rx, &mut carry),
        input("p", "abc")
    );
    assert_eq!(carry, None);
}

#[test]
fn merging_stops_at_another_pane_or_command_and_keeps_order() {
    let (tx, rx) = mpsc::channel();
    tx.send(input("p", "b")).unwrap();
    tx.send(input("q", "x")).unwrap();
    tx.send(input("p", "c")).unwrap();
    let mut carry = None;
    assert_eq!(coalesce(input("p", "a"), &rx, &mut carry), input("p", "ab"));
    assert_eq!(carry, Some(input("q", "x")));
    assert_eq!(rx.try_recv().unwrap(), input("p", "c"));
}

#[test]
fn a_burst_of_resizes_keeps_only_the_last_size() {
    let resize = |rows, cols| Command::Resize {
        pane_id: "p".into(),
        rows,
        cols,
    };
    let (tx, rx) = mpsc::channel();
    tx.send(resize(25, 81)).unwrap();
    tx.send(resize(30, 100)).unwrap();
    tx.send(Command::Focus {
        pane_id: "p".into(),
    })
    .unwrap();
    let mut carry = None;
    assert_eq!(coalesce(resize(24, 80), &rx, &mut carry), resize(30, 100));
    assert_eq!(
        carry,
        Some(Command::Focus {
            pane_id: "p".into()
        })
    );
}

#[test]
fn other_commands_pass_through_untouched() {
    let (_tx, rx) = mpsc::channel();
    let focus = Command::Focus {
        pane_id: "p".into(),
    };
    assert_eq!(coalesce(focus.clone(), &rx, &mut None), focus);
}

#[test]
fn requests_use_the_generated_method_names_and_wire_shapes() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let subscribe = body(Command::Subscribe {
        pane_id: "p".into(),
    });
    assert_eq!(subscribe["method"], "pane.stream.subscribe");
    assert_eq!(
        subscribe["params"],
        serde_json::json!({"paneId": "p", "from": "tail"})
    );
    let resize = body(Command::Resize {
        pane_id: "p".into(),
        rows: 24,
        cols: 80,
    });
    assert_eq!(resize["method"], "pane.resize");
    assert_eq!(
        (
            resize["params"]["rows"].clone(),
            resize["params"]["cols"].clone()
        ),
        (24.into(), 80.into())
    );
    let create = body(Command::CreateTerminal {
        workspace_id: "w".into(),
        cwd: None,
        ticket: 7,
    });
    assert_eq!(create["method"], "pane.create");
    assert_eq!(create["params"]["mode"], "terminal");
    assert!(
        create["params"]["idempotencyKey"]
            .as_str()
            .is_some_and(|key| key.len() == 36)
    );
}

#[test]
fn a_pane_is_created_in_the_folder_it_is_asked_for() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let terminal = body(Command::CreateTerminal {
        workspace_id: "w".into(),
        cwd: Some("/home/ana/notes".into()),
        ticket: 1,
    });
    assert_eq!(terminal["params"]["cwd"], "/home/ana/notes");
    let agent = body(Command::CreateAgent {
        workspace_id: "w".into(),
        harness: "claude".into(),
        cwd: Some("/home/ana/notes".into()),
        ticket: 2,
    });
    assert_eq!(agent["params"]["cwd"], "/home/ana/notes");
    let default = body(Command::CreateTerminal {
        workspace_id: "w".into(),
        cwd: None,
        ticket: 3,
    });
    assert!(default["params"].get("cwd").is_none());
}

#[test]
fn a_web_pane_is_opened_sent_somewhere_and_reported_with_the_engines_methods() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let open = Command::OpenWeb {
        workspace_id: "w".into(),
        url: Some("https://example.com/".into()),
        incognito: false,
        ticket: 4,
    };
    assert_eq!(open.ticket(), Some(4));
    let open = body(open);
    assert_eq!(open["method"], "pane.create");
    assert_eq!(open["params"]["mode"], "web");
    assert_eq!(open["params"]["url"], "https://example.com/");
    assert!(open["params"].get("incognito").is_none());
    let blank = body(Command::OpenWeb {
        workspace_id: "w".into(),
        url: None,
        incognito: true,
        ticket: 5,
    });
    assert!(blank["params"].get("url").is_none());
    assert_eq!(blank["params"]["incognito"], true);

    let go = body(Command::NavigateWeb {
        pane_id: "p".into(),
        url: None,
        action: Some(soyeht_protocol::WebNavigationAction::Back),
    });
    assert_eq!(go["method"], "web.navigate");
    assert_eq!(go["params"]["action"], "back");

    let report = body(Command::ReportWebPage {
        pane_id: "p".into(),
        generation: 12,
        url: "https://example.com/".into(),
        title: "Example".into(),
    });
    assert_eq!(report["method"], "web.host.navigated");
    assert_eq!(report["params"]["navigationGeneration"], "12");
}

#[test]
fn a_workspace_is_made_from_a_folder_and_renamed_with_the_engines_methods() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let made = Command::CreateWorkspace {
        name: "notes".into(),
        path: "/home/ana/notes".into(),
        ticket: 4,
    };
    assert_eq!(made.ticket(), Some(4));
    let sent = body(made.clone());
    assert_eq!(sent["method"], "workspace.create");
    assert_eq!(sent["params"]["name"], "notes");
    assert_eq!(sent["params"]["path"], "/home/ana/notes");
    let answer = crate::testing::workspace_json("w9");
    let event = outcome(&made, serde_json::json!({ "workspace": answer })).unwrap();
    assert!(matches!(
        event,
        Some(Event::WorkspaceOpened { ticket: 4, workspace }) if workspace.workspace_id == "w9"
    ));
    let renamed = body(Command::RenameWorkspace {
        workspace_id: "w".into(),
        name: "Docs".into(),
    });
    assert_eq!(renamed["method"], "workspace.rename");
    assert_eq!(renamed["params"]["workspaceId"], "w");
    assert_eq!(renamed["params"]["name"], "Docs");
    let pane = body(Command::RenamePane {
        pane_id: "p".into(),
        name: "Build".into(),
    });
    assert_eq!(pane["method"], "pane.rename");
    assert_eq!(pane["params"]["paneId"], "p");
    assert_eq!(pane["params"]["name"], "Build");
}

#[test]
fn the_queue_the_question_and_the_sign_in_use_the_engines_methods() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let remove = body(Command::RemoveQueued {
        pane_id: "p".into(),
        prompt_id: "q".into(),
    });
    assert_eq!(remove["method"], "pane.queue.remove");
    assert_eq!(remove["params"]["promptId"], "q");
    let resume = body(Command::ResumeQueue {
        pane_id: "p".into(),
    });
    assert_eq!(resume["method"], "pane.queue.resume");
    let answer = body(Command::AnswerElicitation {
        pane_id: "p".into(),
        request_id: "e".into(),
        fields: Some(serde_json::json!({"a": "b"})),
        cancel: false,
    });
    assert_eq!(answer["method"], "pane.elicitation.answer");
    assert_eq!(answer["params"]["fields"]["a"], "b");
    assert!(answer["params"].get("cancel").is_none());
    let declined = body(Command::AnswerElicitation {
        pane_id: "p".into(),
        request_id: "e".into(),
        fields: None,
        cancel: true,
    });
    assert_eq!(declined["params"]["cancel"], true);
    assert!(declined["params"].get("fields").is_none());
    let start = body(Command::StartSignIn {
        pane_id: "p".into(),
        method_id: Some("m".into()),
    });
    assert_eq!(start["method"], "pane.auth.start");
    assert_eq!(start["params"]["authMethodId"], "m");
    let cancel = body(Command::CancelSignIn {
        pane_id: "p".into(),
    });
    assert_eq!(cancel["method"], "pane.auth.cancel");
    let older = body(Command::LoadOlder {
        pane_id: "p".into(),
        before: "40".into(),
    });
    assert_eq!(older["method"], "pane.transcript");
    assert_eq!(older["params"]["before"], "40");
    assert_eq!(
        older["params"]["limit"],
        soyeht_protocol::client_policy::PAGE_SIZE_TRANSCRIPT
    );
    assert!(
        request(&Command::LoadQueue {
            pane_id: "p".into()
        })
        .is_none()
    );
}

#[test]
fn the_loop_the_role_and_the_draft_use_the_engines_methods() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let set = Command::SetLoop {
        pane_id: "p".into(),
        prompt: "go".into(),
        interval: 300,
        max_rounds: 25,
        stop_when: LoopStopWhen::RoundLimit,
        enabled: true,
    };
    let sent = body(set.clone());
    assert_eq!(sent["method"], "loop.set");
    assert_eq!(sent["params"]["stopWhen"], "round_limit");
    assert_eq!(sent["params"]["maxRounds"], 25);
    let config = serde_json::json!({"prompt": "go", "interval": 300, "maxRounds": 25,
                                    "stopWhen": "round_limit", "enabled": true});
    let event = outcome(&set, serde_json::json!({"config": config})).unwrap();
    assert!(matches!(event, Some(Event::LoopSet { pane_id, .. }) if pane_id == "p"));
    let role = body(Command::SetRole {
        pane_id: "p".into(),
        role: AgentPaneRole::Chief,
    });
    assert_eq!(
        (role["method"].as_str(), role["params"]["role"].as_str()),
        (Some("pane.role.set"), Some("chief"))
    );
    let draft = body(Command::SetDraft {
        pane_id: "p".into(),
        active: true,
    });
    assert_eq!(
        (
            draft["method"].as_str(),
            draft["params"]["active"].as_bool()
        ),
        (Some("pane.draft"), Some(true))
    );
}

#[test]
fn a_page_of_the_transcript_carries_its_cursor() {
    let load = Command::LoadOlder {
        pane_id: "p".into(),
        before: "40".into(),
    };
    let page = serde_json::json!({"items": [], "nextCursor": "20"});
    let event = outcome(&load, page).unwrap();
    assert!(matches!(
        event,
        Some(Event::Transcript { next_cursor: Some(cursor), .. }) if cursor == "20"
    ));
}

#[test]
fn only_fetches_run_in_the_background_and_only_opening_a_pane_has_a_ticket() {
    assert!(Command::LoadUsage.is_background());
    assert!(
        Command::LoadTranscript {
            pane_id: "p".into()
        }
        .is_background()
    );
    assert!(!input("p", "x").is_background());
    let open = Command::CreateTerminal {
        workspace_id: "w".into(),
        cwd: None,
        ticket: 3,
    };
    assert_eq!(open.ticket(), Some(3));
    assert_eq!(input("p", "x").ticket(), None);
}

#[test]
fn the_answer_to_opening_a_pane_carries_its_ticket() {
    let open = Command::CreateTerminal {
        workspace_id: "w".into(),
        cwd: None,
        ticket: 3,
    };
    let event = outcome(&open, serde_json::json!({"paneId": "p9"})).unwrap();
    assert!(matches!(event, Some(Event::Opened { ticket: 3, pane_id }) if pane_id == "p9"));
}

#[test]
fn input_is_base64_with_a_fresh_idempotency_key_each_time() {
    let first = serde_json::to_value(input_request("p", b"ls\r")).unwrap();
    let second = serde_json::to_value(input_request("p", b"ls\r")).unwrap();
    assert_eq!(first["method"], "pane.input");
    assert_eq!(first["params"]["bytes"], "bHMN");
    assert_ne!(
        first["params"]["idempotencyKey"],
        second["params"]["idempotencyKey"]
    );
}

#[test]
fn the_tasks_use_the_engines_methods() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let created = body(Command::CreateTask {
        task: Box::new(NewTask {
            workspace_id: "w".into(),
            title: "t".into(),
            description: "d".into(),
            how_to_verify: None,
            must_not_change: "m".into(),
            executor: None,
            priority: TaskPriority::High,
            create_worktree: true,
            dependencies: vec!["WS-1".into()],
            dispatch: true,
        }),
        ticket: 4,
    });
    assert_eq!(created["method"], "task.create");
    assert_eq!(created["params"]["dispatch"], true);
    assert_eq!(created["params"]["priority"], "high");
    assert_eq!(
        created["params"]["dependencies"],
        serde_json::json!(["WS-1"])
    );
    assert!(created["params"].get("howToVerify").is_none());
    let moved = body(Command::MoveTask {
        workspace_id: "w".into(),
        task_id: "WS-1".into(),
        state: TaskState::InReview,
        reason: soyeht_protocol::client_policy::TASK_MOVE_REASON.into(),
    });
    assert_eq!(
        (moved["method"].as_str(), moved["params"]["state"].as_str()),
        (Some("task.move"), Some("in_review"))
    );
    let preset = TaskExecutor {
        workspace_default: true,
        harness: None,
        model: None,
        effort: None,
        permission_mode: None,
    };
    let dispatch = Command::DispatchTask {
        workspace_id: "w".into(),
        task_id: "WS-1".into(),
        preset,
        ticket: 5,
    };
    assert_eq!(body(dispatch.clone())["method"], "task.dispatch");
    assert_eq!(dispatch.ticket(), Some(5));
    assert_eq!(
        body(Command::LoadTask {
            workspace_id: "w".into(),
            task_id: "WS-1".into()
        })["method"],
        "task.show"
    );
    assert!(
        request(&Command::LoadTasks {
            workspace_id: "w".into()
        })
        .is_none()
    );
    assert!(
        Command::LoadTasks {
            workspace_id: "w".into()
        }
        .is_background()
    );
}

#[test]
fn the_switch_carries_the_model_and_effort_the_person_chose() {
    let body = |model: Option<&str>, effort: Option<&str>| -> Value {
        serde_json::to_value(
            request(&Command::SwitchHarness {
                pane_id: "p".into(),
                harness: "codex".into(),
                model: model.map(str::to_owned),
                effort: effort.map(str::to_owned),
            })
            .unwrap(),
        )
        .unwrap()
    };
    let chosen = body(Some("gpt-6"), Some("high"));
    assert_eq!(chosen["method"], "pane.harness.switch");
    assert_eq!(chosen["params"]["harness"], "codex");
    assert_eq!(chosen["params"]["model"], "gpt-6");
    assert_eq!(chosen["params"]["effort"], "high");
    let left_to_the_harness = body(None, None);
    assert!(left_to_the_harness["params"].get("model").is_none());
    assert!(left_to_the_harness["params"].get("effort").is_none());
}

#[test]
fn account_commands_use_the_engines_auth_methods() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let start = body(Command::StartSignIn {
        pane_id: "p".into(),
        method_id: Some("login".into()),
    });
    assert_eq!(start["method"], "pane.auth.start");
    assert_eq!(start["params"]["authMethodId"], "login");
    let first = body(Command::StartSignIn {
        pane_id: "p".into(),
        method_id: None,
    });
    assert!(first["params"].get("authMethodId").is_none());
    assert_eq!(
        body(Command::CancelSignIn {
            pane_id: "p".into()
        })["method"],
        "pane.auth.cancel"
    );
    assert_eq!(
        body(Command::SignOut {
            pane_id: "p".into()
        })["method"],
        "pane.auth.logout"
    );
}

#[test]
fn settings_are_written_under_the_keys_the_engine_names() {
    let body =
        |command: Command| -> Value { serde_json::to_value(request(&command).unwrap()).unwrap() };
    let preferred = body(Command::SetPreferredHarness {
        harness: "codex".into(),
    });
    assert_eq!(preferred["method"], "settings.set");
    assert_eq!(preferred["params"]["key"], "harness.preferredId");
    assert_eq!(preferred["params"]["value"], "codex");
    let mode = body(Command::SetDefaultMode {
        harness: "codex".into(),
        mode: None,
    });
    assert_eq!(mode["params"]["key"], "harness.defaultMode.codex");
    assert_eq!(
        mode["params"]["value"], "",
        "no mode keeps the harness's own"
    );
}

#[test]
fn a_rescan_and_a_model_page_are_ordinary_requests() {
    let rescan =
        serde_json::to_value(request(&Command::LoadHarnesses { rescan: true }).unwrap()).unwrap();
    assert_eq!(rescan["params"]["rescan"], true);
    let plain =
        serde_json::to_value(request(&Command::LoadHarnesses { rescan: false }).unwrap()).unwrap();
    assert!(plain["params"].get("rescan").is_none());
    let page = serde_json::to_value(crate::preference_requests::catalogue_request(
        "codex",
        Some("c2".into()),
    ))
    .unwrap();
    assert_eq!(page["method"], "model.list");
    assert_eq!(page["params"]["cursor"], "c2");
}
