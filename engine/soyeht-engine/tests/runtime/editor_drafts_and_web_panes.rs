use super::*;

#[test]
fn project_replacement_never_overwrites_unexpected_disk_content() {
    let mut rig = Rig::new();
    let repository = rig.path.join("editor-replace-race");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("note.txt");
    let before = "needle\n";
    fs::write(&path, before).unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Race","path":repository,"idempotencyKey":"race-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let params = json!({
        "workspaceId":workspace,"query":"needle","replacement":"safe",
        "caseSensitive":true,"limit":10,"idempotencyKey":"race-replace"
    });
    rig.kill_engine();
    stage_interrupted_replacement(&rig, &workspace, &params, before, "safe\n");
    {
        let store = Store::open(&rig.path).unwrap();
        let uid = soyeht_platform::user::id().unwrap();
        assert!(
            store
                .file_replacement(&format!("local:{uid}:runtime-test"), "race-replace")
                .unwrap()
                .is_some()
        );
    }
    fs::write(&path, "external\n").unwrap();
    rig.start();
    let response = rig.client().request("file.search", params);
    assert_eq!(
        response["error"]["data"]["kind"], "stale_version",
        "{response}"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "external\n");
    rig.kill_engine();
    let store = Store::open(&rig.path).unwrap();
    assert_eq!(store.active_file_replacements().unwrap().len(), 1);
}

#[test]
fn editor_draft_survives_restart_then_autosaves_versions_and_reverts() {
    let mut rig = Rig::new();
    let repository = rig.path.join("editor-save");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("note.txt");
    fs::write(&path, "alpha\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Save","path":repository,"idempotencyKey":"save-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"note.txt","idempotencyKey":"open-save"}),
    );
    let file_id = opened["file"]["fileId"].as_str().unwrap().to_owned();
    let autosave_pause = rig.path.join("test-pause-editor-autosave");
    fs::write(&autosave_pause, b"pause").unwrap();
    let write_params = json!({
        "fileId":file_id,"baseRevision":"1","author":local_file_author(),
        "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":5}},"text":"bravo"}],
        "idempotencyKey":"write-bravo"
    });
    let write = client.call("file.write", write_params.clone());
    assert_eq!(write["file"]["dirty"], true);
    assert_eq!(client.call("file.write", write_params), write);
    let forged = client.request(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"2",
            "author":{"actor":{"role":"human_local","client":"another-client"},"name":"you"},
            "edits":[],"idempotencyKey":"forged-author"
        }),
    );
    assert_eq!(forged["error"]["data"]["kind"], "unauthorized");
    let stale = client.request(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"1","author":local_file_author(),
            "edits":[],"idempotencyKey":"stale-editor-write"
        }),
    );
    assert_eq!(stale["error"]["data"]["kind"], "stale_version");
    assert_eq!(fs::read_to_string(&path).unwrap(), "alpha\n");
    rig.kill_engine();
    assert_eq!(
        rig.database_probe().editor_draft(&file_id).unwrap(),
        Some("bravo\n".into())
    );
    fs::remove_file(autosave_pause).unwrap();
    rig.start();
    let mut restored = rig.client();
    let saved = wait_for_editor_file(&mut restored, &file_id, |file| file["dirty"] == false);
    assert_eq!(saved["revision"], "2");
    assert_eq!(saved["diskRevision"], "2");
    assert_eq!(fs::read_to_string(&path).unwrap(), "bravo\n");
    let history = restored.call("file.history", json!({"fileId":file_id,"limit":1}));
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert_eq!(history["items"][0]["author"]["name"], "you");
    let bravo_version = history["items"][0]["versionId"].clone();
    let (before_second, _) = restored.snapshot();
    restored.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"2","author":local_file_author(),
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":5}},"text":"charlie"}],
            "idempotencyKey":"write-charlie"
        }),
    );
    wait_for_editor_file(&mut restored, &file_id, |file| {
        file["dirty"] == false && file["revision"] == "3"
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "charlie\n");
    restored.messages.clear();
    restored.call("events.subscribe", json!({"since":before_second["cursor"]}));
    assert!(restored.messages.iter().any(|frame| {
        let Frame::Json(bytes) = frame else {
            return false;
        };
        let value: Value = serde_json::from_slice(bytes).unwrap();
        value["method"] == "file.saved" && value["params"]["by"]["name"] == "you"
    }));
    restored.call(
        "file.revert",
        json!({
            "fileId":file_id,"versionId":bravo_version,"baseRevision":"3",
            "idempotencyKey":"revert-bravo"
        }),
    );
    wait_for_editor_file(&mut restored, &file_id, |file| {
        file["dirty"] == false && file["revision"] == "4"
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "bravo\n");
    assert_eq!(
        restored.call("file.history", json!({"fileId":file_id,"limit":10}))["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn editor_disk_reload_and_conflict_preserve_both_versions_until_resolution() {
    let rig = Rig::new();
    let repository = rig.path.join("editor-conflict");
    fs::create_dir_all(&repository).unwrap();
    let path = repository.join("note.txt");
    fs::write(&path, "base\n").unwrap();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Conflict","path":repository,"idempotencyKey":"conflict-workspace"}),
    )["workspace"]["workspaceId"]
        .clone();
    let opened = client.call(
        "file.open",
        json!({"workspaceId":workspace,"path":"note.txt","idempotencyKey":"open-conflict"}),
    );
    let file_id = opened["file"]["fileId"].as_str().unwrap().to_owned();
    let (before_disk_changes, _) = client.snapshot();
    client.call(
        "file.watch",
        json!({"fileId":file_id,"watch":false,"idempotencyKey":"watch-off"}),
    );
    fs::write(&path, "outside\n").unwrap();
    thread::sleep(Duration::from_millis(400));
    assert_eq!(client.section("files")[0]["revision"], "1");
    client.call(
        "file.watch",
        json!({"fileId":file_id,"watch":true,"idempotencyKey":"watch-on"}),
    );
    let reloaded = wait_for_editor_file(&mut client, &file_id, |file| file["revision"] == "2");
    assert_eq!(reloaded["dirty"], false);
    let disk = client.call(
        "file.read",
        json!({"fileId":file_id,"source":"disk","fromLine":0,"lineCount":1}),
    );
    assert_eq!(disk["lines"][0]["text"], "outside");
    client.call(
        "file.watch",
        json!({"fileId":file_id,"watch":false,"idempotencyKey":"conflict-watch-off"}),
    );
    fs::write(&path, "second external\n").unwrap();
    client.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"2","author":local_file_author(),
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":7}},"text":"draft"}],
            "idempotencyKey":"write-draft"
        }),
    );
    client.call(
        "file.watch",
        json!({"fileId":file_id,"watch":true,"idempotencyKey":"conflict-watch-on"}),
    );
    let conflicted = wait_for_editor_file(&mut client, &file_id, |file| file["conflicted"] == true);
    assert_eq!(conflicted["revision"], "3");
    thread::sleep(Duration::from_millis(600));
    assert_eq!(fs::read_to_string(&path).unwrap(), "second external\n");
    let draft = client.call(
        "file.read",
        json!({"fileId":file_id,"source":"draft","fromLine":0,"lineCount":1}),
    );
    let disk = client.call(
        "file.read",
        json!({"fileId":file_id,"source":"disk","fromLine":0,"lineCount":1}),
    );
    assert_eq!(draft["lines"][0]["text"], "draft");
    assert_eq!(disk["lines"][0]["text"], "second external");
    client.messages.clear();
    client.call(
        "events.subscribe",
        json!({"since":before_disk_changes["cursor"]}),
    );
    let event_names: Vec<_> = client
        .messages
        .iter()
        .filter_map(|frame| {
            let Frame::Json(bytes) = frame else {
                return None;
            };
            serde_json::from_slice::<Value>(bytes).ok()?["method"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    assert!(event_names.contains(&"file.changed_on_disk".to_owned()));
    assert!(event_names.contains(&"file.conflict".to_owned()));
    client.call(
        "file.write",
        json!({
            "fileId":file_id,"baseRevision":"3","author":local_file_author(),
            "edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":5}},"text":"chosen"}],
            "idempotencyKey":"resolve-conflict"
        }),
    );
    wait_for_editor_file(&mut client, &file_id, |file| {
        file["conflicted"] == false && file["dirty"] == false
    });
    assert_eq!(fs::read_to_string(&path).unwrap(), "chosen\n");
}

#[test]
fn creation_retries_and_close_survive_engine_restart_without_resurrection() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let (workspace, pane) = client.create(&rig);
    let duplicate = client.call(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"terminal","idempotencyKey":"create-pane"}),
    );
    assert_eq!(duplicate["paneId"], pane);
    let pid = client.pane(&pane)["pid"].clone();
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.pane(&pane)["pid"], pid);
    let close = json!({"paneId":pane,"idempotencyKey":"close-pane"});
    client.call("pane.close", close.clone());
    assert_eq!(client.call("pane.close", close.clone())["paneId"], pane);
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.call("pane.close", close)["paneId"], pane);
    assert!(
        client
            .snapshot()
            .1
            .iter()
            .filter(|p| p["section"] == "panes")
            .all(|p| p["data"].as_array().unwrap().is_empty())
    );
    let changed=client.request("pane.create",json!({"workspaceId":workspace,"mode":"terminal","name":"Different","idempotencyKey":"create-pane"}));
    assert_eq!(changed["error"]["data"]["kind"], "idempotency_conflict");
}

#[test]
fn unsigned_local_client_cannot_change_experiment_consent() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let (_, pane) = client.create(&rig);
    let before = client.pane(&pane);
    assert_eq!(before["experimentCapture"]["on"], false);
    let replay_recording = before["record"].clone();

    let refused = client.request(
        "pane.experiment.set",
        json!({
            "paneId":pane,"on":true,"idempotencyKey":"unsigned-experiment-capture"
        }),
    );
    assert_eq!(refused["error"]["data"]["kind"], "forbidden_role");
    assert_eq!(client.pane(&pane)["experimentCapture"]["on"], false);
    assert_eq!(client.pane(&pane)["record"], replay_recording);

    let delete_refused = client.request(
        "pane.experiment.delete",
        json!({"paneId":pane,"idempotencyKey":"unsigned-experiment-delete"}),
    );
    assert_eq!(delete_refused["error"]["data"]["kind"], "forbidden_role");
    let export_refused = client.request(
        "pane.experiment.export",
        json!({
            "paneIds":[pane],"fromMs":"0","toMs":"1",
            "idempotencyKey":"unsigned-experiment-export"
        }),
    );
    assert_eq!(export_refused["error"]["data"]["kind"], "forbidden_role");

    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    let restored = client.pane(&pane);
    assert_eq!(restored["experimentCapture"]["on"], false);
    assert_eq!(restored["record"], replay_recording);
}

#[test]
fn web_panes_are_bounded_durable_and_never_enter_the_process_registry() {
    let mut rig = Rig::new();
    let mut client = rig.client();
    let workspace = client.call(
        "workspace.create",
        json!({"name":"Web","path":rig.path,"idempotencyKey":"web-workspace"}),
    )["workspace"]["workspaceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let first_request = json!({
        "workspaceId":workspace,"mode":"web","url":"localhost:5173",
        "incognito":true,"idempotencyKey":"web-pane-0"
    });
    let first = client.call("pane.create", first_request.clone())["paneId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(client.call("pane.create", first_request)["paneId"], first);
    let pane = client.pane(&first);
    assert_eq!(pane["mode"], "web");
    assert_eq!(pane["url"], "http://localhost:5173/");
    assert_eq!(pane["webSession"], "incognito");
    assert!(pane["pid"].is_null() && pane["streamId"].is_null());
    for index in 1..10 {
        client.call(
            "pane.create",
            json!({
                "workspaceId":workspace,"mode":"web",
                "idempotencyKey":format!("web-pane-{index}")
            }),
        );
    }
    let overflow = client.request(
        "pane.create",
        json!({"workspaceId":workspace,"mode":"web","idempotencyKey":"web-pane-10"}),
    );
    assert_eq!(overflow["error"]["data"]["kind"], "invalid_params");
    assert!(process_instances(&rig).is_empty());
    drop(client);
    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    assert_eq!(client.pane(&first)["url"], "http://localhost:5173/");
    assert!(process_instances(&rig).is_empty());
    client.call(
        "pane.close",
        json!({"paneId":first,"idempotencyKey":"close-web-pane"}),
    );
    assert!(
        !client
            .section("panes")
            .iter()
            .any(|pane| pane["paneId"] == first)
    );
}
